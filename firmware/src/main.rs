#![no_std]
#![no_main]

mod boot;
mod init;
mod last_reset;
mod led;
mod log;
mod pins;
mod settings;
mod sync;
mod ww;

use cnt::cnt; // cnt::bkp_cnt! for counters in backup registers
use defmt::{error, info, unwrap};

use cortex_m_rt::exception;
use embassy_stm32::rcc::{Hse, HseMode, Pll, PllMul, PllPreDiv, PllRDiv, PllSource, Sysclk};
use embassy_stm32::rcc::{Hsi48Config, mux};
use embassy_time::Timer;

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    // First: may reset once (after the ROM bootloader's jump) or jump into the ROM bootloader (src/boot.rs)
    let early = boot::early();
    // RTT channel: the log's tee (src/log.rs)
    log::init_rtt();
    info!("cannify_micro starting...");
    boot::log(&early);
    let last_reset = last_reset::boot(); // reset cause and crash record (src/last_reset.rs), logged
    let mut config = embassy_stm32::Config::default();
    // Clock tree: the 12 MHz crystal Y2 (HSE) feeds the PLL: 12 MHz / 1 * 16 = 192 MHz VCO, R divider 3 = 64 MHz
    // system clock, the chip's maximum. AHB and APB run at 64 MHz too (embassy's default prescalers). The FDCAN
    // kernel clock is the crystal itself, as mcan's hil/b129 runs it: 12 MHz gives exact bit times for the usual
    // rates.
    config.rcc.hse = Some(Hse {
        freq: embassy_stm32::time::Hertz::mhz(12),
        mode: HseMode::Oscillator,
    });
    config.rcc.pll = Some(Pll {
        source: PllSource::HSE,
        prediv: PllPreDiv::DIV1,
        mul: PllMul::MUL16,
        divp: None,
        divq: None,
        divr: Some(PllRDiv::DIV3),
    });
    config.rcc.sys = Sysclk::PLL1_R;
    config.rcc.mux.fdcansel = embassy_stm32::rcc::mux::Fdcansel::HSE;
    // USB clock: HSI48, trimmed by CRS from USB start-of-frame packets
    config.rcc.hsi48 = Some(Hsi48Config {
        sync_from_usb: true,
    });
    config.rcc.mux.usbsel = mux::Usbsel::HSI48;
    let p = embassy_stm32::init(config);
    // Flash prefetch: embassy-stm32 0.6 sets the flash wait states on the G0 but leaves PRFTEN off, so sequential
    // fetches stall with wait states (8-16% slower code at 64 MHz); embassy main turns it on itself (#7276).
    // ES0548 2.2.10: on this dual-bank part prefetch can fail on a branch into the other flash bank, so all code has
    // to stay in bank 1 (memory.x stops the link otherwise); clear PRFTEN instead when the code outgrows it.
    embassy_stm32::pac::FLASH
        .acr()
        .modify(|w| w.set_prften(true));
    init::init();
    boot::after_init(spawner, &early); // the boot word again, the rescue after a boot loop, the reboot task

    let settings = settings::SettingsStore::load(p.FLASH).await;
    info!("label: {}", settings.get().label);

    // D14 breathes, or shines solid with led_at_boot (src/led.rs)
    let led_mode = if settings.get().led_at_boot {
        led::Mode::On
    } else {
        led::DEFAULT_MODE
    };
    spawner.spawn(unwrap!(led::led_task(p.TIM15, p.PB14, led_mode)));
    // every free pin, for `pins` (src/pins.rs): take a pin out of its list there to use it here
    let pins = pins::Pins::new(pins::take!(p));

    let state = ww::ServerState::new(last_reset, settings, pins).await;
    let driver = ww::usb_driver(p.USB, p.PA12, p.PA11);
    ww::start(spawner, driver, state);

    info!("init done");
    loop {
        cnt!(loop_iterations: u32);
        Timer::after_millis(1000).await;
    }
}

#[exception]
unsafe fn DefaultHandler(irqn: i16) {
    cnt!(unhandled_exceptions: u32, warn);
    cnt::bkp_cnt!(unhandled_exceptions_total: u32, warn);
    error!("unhandled exception, IRQn = {}", irqn);
}

#[exception]
unsafe fn NonMaskableInt() {
    // a flash word with an ECC error, read by the settings store (a power cut while it was written): it reads as
    // zeros, the store skips it
    if ww_ecc_flash::on_nmi() {
        return;
    }
    panic!("NMI: not a flash ECC error of a settings read");
}

#[exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    let exc_return = bedrock_crash::exc_return!(); // first: lr still holds EXC_RETURN here
    // Saved for the next boot (src/last_reset.rs, served as LastReset). With `probe-rs run` the debugger
    // stops at the fault before this handler runs.
    unsafe { bedrock_crash::capture_fault(ef, exc_return, last_reset::uptime_ms()) };
    cnt::bkp_cnt!(hard_faults: u32, error);
    error!("HardFault {}", defmt::Debug2Format(ef));
    bedrock_crash::reset()
}
