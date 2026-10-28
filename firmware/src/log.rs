//! The defmt log: `ww_defmt_logger` is the global logger. Every frame goes to the RTT channel `defmt`, so
//! `probe-rs run` / `cnt run` (`just run`, `just cnt`) show the log as before, and into a ring in RAM that the
//! WireWeaver server task empties ([pump]) into `device.log.frames` (`ww_log::LogDefmt`), a reliable stream whose
//! store sits in `.uninit` RAM, read over USB.
//!
//! The store keeps the log while no host is connected (the stream's session starts at boot, a host resumes it) and
//! across a soft reset: a crash (`HardFault`, panic, both reset) leaves it as it was, and the next boot takes it up
//! again, so the lines before the crash reach the host after it. A power cut loses it. What is still in the logger's
//! ring when the MCU resets (logged since the last [pump], e.g. by the fault handler itself) is lost; the crash
//! record (`device.last_reset`) has the fault.

use core::mem::MaybeUninit;
use defmt::{info, warn};
use rtt_target::{ChannelMode::NoBlockSkip, rtt_init};
use ww_stream::store::{FullPolicy, PersistentRamRing, RamArea};
use ww_stream::tx::Reliable;

/// The ring between the logging code and [pump]: holds what is logged between two pumps (the boot log until the
/// server task runs, at most [PUMP_PERIOD] after that). A frame that doesn't fit is dropped and counted.
const RING_LEN: usize = 4096;
ww_defmt_logger::global_logger!(LOG, RING_LEN);

/// Bytes of log the store keeps until a host acknowledges them; the oldest go first when full.
pub const STORE_LEN: usize = 8192;
pub type Store = PersistentRamRing<'static, STORE_LEN>;
pub type Stream = Reliable<Store>;

/// How often the server task moves the ring into the store.
pub const PUMP_PERIOD: embassy_time::Duration = embassy_time::Duration::from_millis(10);

/// Left alone by the startup code (cortex-m-rt's `.uninit`), so it survives a reset; the store checks it at boot.
#[unsafe(link_section = ".uninit.cannify_micro_log")]
static mut AREA: MaybeUninit<RamArea<STORE_LEN>> = MaybeUninit::uninit();

/// The RTT channel of the log's tee (up 0 `defmt`), for probe-rs / cnt run. Early in main: what is logged before it
/// does not reach RTT (it still goes into the ring). WireWeaver itself runs over USB.
pub fn init_rtt() {
    // Never blocks: the logger writes each frame whole or skips it, in NoBlockSkip even when probe-rs switches the
    // channel to blocking on attach, so the firmware runs the same with or without a probe. A burst faster than
    // the probe reads loses lines on RTT only; device.log has them.
    let channels = rtt_init! {
        up: {
            0: {
                size: 1024,
                mode: NoBlockSkip,
                name: "defmt"
            }
        }
    };
    LOG.set_rtt_channel(channels.up.0);
}

/// The stream of `device.log.frames` on the store kept in `.uninit`: the session of the boot before goes on (its
/// log from before a reset is still there), or a new one starts as `session` when there is none (a power-on).
/// Call once.
pub async fn stream(session: u32) -> Stream {
    // SAFETY: called once (ServerState is built once), nothing else uses AREA
    let area = unsafe { RamArea::from_uninit(&mut *core::ptr::addr_of_mut!(AREA)) };
    let store = PersistentRamRing::attach(area).with_policy(FullPolicy::DropOldest);
    let recovery = store.recovery();
    let mut stream = Reliable::new(store);
    stream.continue_stored_session();
    match stream.session() {
        Some(kept) => info!(
            "log store: session {=u32:x} continued, {=u64} log frames from before, clock epoch {=u32}, {=u32} B cut off",
            kept,
            stream.next_pos(),
            recovery.clock_epoch,
            recovery.truncated_bytes
        ),
        None => {
            if stream.start_session(session).await.is_err() {
                // a RAM store never fails
                warn!("log store: no session");
            }
            info!("log store: new session {=u32:x}", session);
        }
    }
    stream
}

/// Moves what was logged since the last call into the store. Called by the server task every [PUMP_PERIOD] and
/// before it sends the stream.
pub async fn pump(stream: &mut Stream) {
    if LOG.pump(stream).await.is_err() {
        cnt::cnt!(log_store_errors: u32);
    }
}

/// Encoding of the frames, for `device.log.DEFMT`.
pub fn encoding() -> ww_log::DefmtEncoding {
    ww_defmt_logger::encoding()
}
