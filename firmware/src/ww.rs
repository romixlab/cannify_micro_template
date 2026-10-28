//! WireWeaver server: implements `cannify_micro_api::CannifyMicroApi` (defined in `../cannify_micro_api/src/lib.rs`) for
//! [ServerState] and serves it over USB. `led_on` / `led_off` are here, the device contract
//! (`device`) in `ww/device.rs`, D14's indication (`leds`) in `ww/leds.rs`, the clock a host steers (`clock`) in
//! `ww/clock.rs`.
//!
//! To add a method, property or stream: declare it in the API trait, then implement it here, `ww_codegen!` below
//! regenerates the dispatch code and the compiler lists what is missing. Extend [ServerState] with the peripherals
//! and data the implementations need. See https://github.com/vhrdtech/wire_weaver (`docs/`, `examples_mcu/`).

mod clock;
mod device;
mod leds;

use cnt::cnt;
use defmt::{info, unwrap, warn};
use embassy_executor::Spawner;
use embassy_futures::select::{Either3, select3};
use embassy_stm32::{Peri, bind_interrupts, peripherals, usb};
use embassy_time::{Instant, Timer};
use static_cell::StaticCell;
use wire_weaver::prelude::*;
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};
use wire_weaver_usb_embassy::ww_device::LinkEvent;
use wire_weaver_usb_embassy::{LinkConfig, UsbBuffers, UsbDevice, UsbServer, UsbTimings, usb_init};
use ww_pin_diag::{Level, PinDiagError, PointState};
use ww_uid::{UidKind, UidSource, UniqueId};

/// Longest WireWeaver message the device accepts and can reply with (reported to the host)
const MAX_MESSAGE_LEN: usize = 1024;

/// Biggest item of a reliable stream: `device.log.frames`' `DefmtFrames` (512 B of frames and its length).
const STREAM_BUF_LEN: usize = ww_log::MAX_FRAMES_LEN + 16;
/// Bytes of stream items sent per turn of the server task (at least one event), so requests keep being served.
const STREAM_BUDGET: usize = 2048;

/// Everything the API implementation needs: peripherals, drivers, state.
pub struct ServerState {
    /// Reset cause and crash record read at boot (src/last_reset.rs)
    last_reset: bedrock_crash::Snapshot,
    /// Settings in flash (src/settings.rs): `device.label`, `device.settings`
    settings: crate::settings::SettingsStore,
    /// `device.log.frames`: the defmt log (src/log.rs)
    log_frames: crate::log::Stream,
    /// `device.uid.IDS`: the STM32 96-bit unique id
    uid: [UniqueId<'static>; 1],
    /// `device.counters.read`: one page of counter words, copied at once
    counters_page: [UVlq32; ww_counters::MAX_WORDS],
    /// Sessions of reliable streams opened so far, see [Self::new_stream_session]
    stream_sessions: u32,
    /// `pins`: the free pins (src/pins.rs)
    pins: crate::pins::Pins,
    /// `leds`: mode, brightness and the end of a running identify (src/led.rs shows them)
    leds: leds::LedsState,
    /// `clock`: the sync line's configuration (always `Off`) and whether `time_map` is subscribed; the shared clock
    /// itself is `crate::sync::SYNC`, read by the LED task too
    clock: clock::ClockState,
}

/// The chip's unique id, read once
static UID: StaticCell<[u8; 12]> = StaticCell::new();

impl ServerState {
    pub async fn new(
        last_reset: bedrock_crash::Snapshot,
        settings: crate::settings::SettingsStore,
        pins: crate::pins::Pins,
    ) -> Self {
        // the boot count in the high half: a session of this boot never takes a host's cursor of an earlier boot
        // for its own (the boot count restarts at a power-on, the log store is lost with it then)
        let stream_sessions = last_reset.view().boot_count << 16 | 1;
        ServerState {
            last_reset,
            settings,
            log_frames: crate::log::stream(stream_sessions).await,
            uid: [UniqueId {
                kind: UidKind::ChipUnique,
                source: UidSource::InternalIc,
                value: MaxLen::new_unchecked(RefVec::new_bytes(
                    UID.init(embassy_stm32::uid::uid()),
                )),
            }],
            counters_page: [UVlq32(0); ww_counters::MAX_WORDS],
            stream_sessions,
            pins,
            leds: leds::LedsState::new(),
            clock: clock::ClockState::new(),
        }
    }

    /// Number of a new session of a reliable stream, when a host opens one anew.
    fn new_stream_session(&mut self) -> u32 {
        // the first one went to the log at boot (`ServerState::new`)
        self.stream_sessions = self.stream_sessions.wrapping_add(1);
        self.stream_sessions
    }
}

// Implementations of the CannifyMicroApi methods. `method_model = "_=immediate"`: they return a result right away
// (`RpcResult` can also defer the reply, see wire_weaver::RpcResult). Plain `fn`s unless they await
// (`handler_model` below).
impl ServerState {
    fn led_on(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        info!("led on");
        cnt!(led_on_calls: u32);
        crate::led::set(crate::led::Mode::On);
        Ready(())
    }

    fn led_off(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        info!("led off");
        cnt!(led_off_calls: u32);
        crate::led::set(crate::led::Mode::Off);
        Ready(())
    }
}

// ww_pin_diag::PinDiag (`pins`): raw access to the free pins (src/pins.rs). A refusal is counted and logged.
impl ServerState {
    fn pins_set(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        point: u16,
        state: PointState,
    ) -> RpcResult<Result<(), PinDiagError<'_>>> {
        Ready(
            self.pins
                .set(point, state)
                .inspect_err(|e| refused(point, e)),
        )
    }

    fn pins_read(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        point: u16,
    ) -> RpcResult<Result<Level, PinDiagError<'_>>> {
        Ready(self.pins.read(point).inspect_err(|e| refused(point, e)))
    }

    fn pins_release_all(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        if self.pins.release_all() {
            info!("pins released");
        }
        Ready(())
    }
}

fn refused(point: u16, e: &PinDiagError<'_>) {
    cnt!(pin_requests_refused: u32);
    warn!("pins: point {} refused: {}", point, e);
}

mod server_impl {
    // Re-parses the API trait and generates request dispatch + serdes for ServerState.
    // To see the generated code, add: debug_to_file = "./target/ww_server.rs"
    wire_weaver::ww_codegen!(
        cannify_micro_api :: CannifyMicroApi for super::ServerState,
        server = true, no_alloc = true, use_async = true,
        // handlers that await (flash writes, clock.steer's time_map point) are async, all others plain `fn`s: an
        // `async fn` that never awaits still costs flash as a state of the dispatcher
        handler_model = "device.label.*|device.settings.*|clock.steer=async, _=sync",
        method_model = "_=immediate",
        property_model = "_=get_set",
        introspect = "with_docs",
        // the board record and the chip ids are read at run time, the build id and the counter buffers are known at
        // link time, the defmt encoding at run time (ww/device.rs); the free pins are a table in flash (src/pins.rs)
        const_model = "BOARD|IDS|INFO|BUILD_ID|DEFMT|POINTS=fn",
        // left out: the optional build record, the board's reworks list, label notes (no room in the settings),
        // counters.watch (a host polls `read` over USB)
        omit = [
            "device.firmware.BUILD_RECORD",
            "device.board.REWORKS",
            "device.label.notes",
            "device.counters.watch",
        ],
    );
}

impl WireWeaverAsyncApiBackend for ServerState {
    type Medium = ();

    async fn process_bytes<'a>(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], shrink_wrap::Error> {
        self.process_request_bytes(data, scratch, out, medium).await
    }

    fn version(&self) -> FullVersion<'_> {
        cannify_micro_api::CANNIFY_MICRO_API_FULL_GID
    }
}

fn link_config() -> LinkConfig<'static> {
    LinkConfig::new(
        cannify_micro_api::CANNIFY_MICRO_API_FULL_GID,
        server_impl::api_hash(),
        ww_client_server::COMPACT_VERSION,
    )
}

bind_interrupts!(struct Irqs {
    USB_UCPD1_2 => usb::InterruptHandler<peripherals::USB>;
});

pub type UsbDriver = usb::Driver<'static, peripherals::USB>;

const MAX_USB_PACKET_LEN: usize = 64; // Full Speed
static USB_BUFFERS: StaticCell<UsbBuffers<MAX_USB_PACKET_LEN, MAX_MESSAGE_LEN>> = StaticCell::new();

pub fn usb_driver(
    usb: Peri<'static, peripherals::USB>,
    dp: Peri<'static, peripherals::PA12>,
    dm: Peri<'static, peripherals::PA11>,
) -> UsbDriver {
    usb::Driver::new(usb, Irqs, dp, dm)
}

pub fn start(spawner: Spawner, driver: UsbDriver, state: ServerState) {
    // server_impl::API_ID is reported in a USB string descriptor, `ww list` shows it without opening the device.
    // A user label (e.g. loaded from flash) can be added with wire_weaver::api_id::with_label().
    let (usb, server) = usb_init(
        driver,
        USB_BUFFERS.init(UsbBuffers::default()),
        UsbTimings::fs_higher_speed(),
        link_config(),
        server_impl::API_ID,
        |config| {
            // VID:PID is c0de:cafe unless set here
            config.product = Some("cannify_micro");
            config.serial_number = Some(embassy_stm32::uid::uid_hex());
        },
    );
    spawner.spawn(unwrap!(usb_task(usb)));
    spawner.spawn(unwrap!(server_task(server, state)));
}

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, UsbDriver>) {
    usb.run().await;
}

/// Serves requests; every [PUMP_PERIOD](crate::log::PUMP_PERIOD) moves the defmt log into its store (src/log.rs) and
/// sends what the log stream has to a host that opened it; writes the settings once they settle, also while no host
/// is connected (a change made just before it left still reaches the flash). The log stays in its store while no
/// host is connected, a host that comes back resumes it.
#[embassy_executor::task]
async fn server_task(mut server: UsbServer<'static, UsbDriver>, mut state: ServerState) {
    let mut stream_buf = [0u8; STREAM_BUF_LEN];
    let mut next_pump = Instant::now() + crate::log::PUMP_PERIOD;
    loop {
        match select3(
            server.wait(),
            Timer::at(next_pump),
            Timer::at(state.settings.due_at()),
        )
        .await
        {
            Either3::First(ready) => {
                if let Some(event) = server.handle(ready, &mut state).await {
                    info!("{:?}", event);
                    if matches!(event, LinkEvent::Down(_)) {
                        state.log_frames.detach();
                        // a host that disconnects or goes silent gives the pins back too
                        if state.pins.release_all() {
                            info!("pins released");
                        }
                    }
                }
            }
            Either3::Second(()) => next_pump += crate::log::PUMP_PERIOD,
            Either3::Third(()) => state.settings.persist_if_due().await,
        }
        crate::log::pump(&mut state.log_frames).await;
        if server.is_up()
            && state
                .poll_streams(
                    &mut server.sink(),
                    Instant::now().as_millis(),
                    &mut stream_buf,
                    STREAM_BUDGET,
                )
                .await
                .is_err()
        {
            cnt!(stream_send_errors: u32);
        }
    }
}
