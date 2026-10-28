//! Crash capture (bedrock_crash): the `HardFault` handler in main.rs and the panic handler below save a record
//! (cause, uptime, registers, 32 stack words, message, build id) that survives the reset; [boot] reads it at the
//! next start together with the reset cause. `ww.rs` serves it as `ww_firmware_info::LastReset`.
//! The record lives in `.uninit` RAM: it survives resets, not power loss.

use bedrock_crash::stm32::ResetFlags;
use bedrock_crash::{Cause, Snapshot};
use defmt::{error, info};
use wire_weaver::prelude::{MaxLen, RefVec};
use ww_firmware_info::{CortexMFault, Fault, ResetCause, ResetRecord, Second};

/// Read the reset cause and the crash record, count the boot and log it. Call it once, early in main.
pub fn boot() -> Snapshot {
    let snapshot = bedrock_crash::boot(reset_cause(), build_id());
    let v = snapshot.view();
    info!("last reset: {}, boot {}", v.cause, v.boot_count);
    if let Some(c) = v.crash.filter(|c| c.cause == v.cause) {
        let at = c.uptime_ms.unwrap_or(0);
        error!("before it: {} at {} ms", c.message, at);
        if let Some(regs) = c.regs {
            error!("{}", regs);
        }
    }
    snapshot
}

/// The reset flags in RCC_CSR (stm32-data `rcc_g0x1`), cleared with RMVF so the next reset starts
/// fresh.
fn reset_cause() -> Cause {
    const RCC_CSR: *mut u32 = 0x40021060 as *mut u32;
    // SAFETY: an RCC register, read once at boot
    let v = unsafe { RCC_CSR.read_volatile() };
    let bit = |n: u32| v & (1 << n) != 0;
    let flags = ResetFlags {
        power_on: bit(27),             // PWRRSTF
        pin: bit(26),                  // PINRSTF
        independent_watchdog: bit(29), // IWDGRSTF
        window_watchdog: bit(30),      // WWDGRSTF
        software: bit(28),             // SFTRSTF
        low_power: bit(31),            // LPWRRSTF
        option_bytes: bit(25),         // OBLRSTF
        ..ResetFlags::default()
    };
    // SAFETY: sets RMVF, writes the other bits back unchanged
    unsafe { RCC_CSR.write_volatile(v | 1 << 23) };
    flags.cause()
}

/// Time since boot, saved with a crash; called from the fault and panic handlers.
pub fn uptime_ms() -> Option<u64> {
    Some(embassy_time::Instant::now().as_millis())
}

/// Build id of this image, saved with a crash so the host finds the ELF that crashed.
fn build_id() -> &'static [u8] {
    ww_firmware_info::build_id()
}

/// Saves the panic for the next boot, logs it and resets (with a debugger attached: stops, as panic-probe does).
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    bedrock_crash::capture_panic(info, uptime_ms());
    error!("{}", defmt::Display2Format(info));
    bedrock_crash::panic_reset()
}

/// The record as `LastReset::last_reset` reports it (src/ww.rs).
pub fn record(snapshot: &Snapshot) -> ResetRecord<'_> {
    let v = snapshot.view();
    let crash = v.crash;
    ResetRecord {
        cause: ww_cause(v.cause),
        build_id: MaxLen::new_unchecked(RefVec::new_bytes(crash.map_or(&[][..], |c| c.build_id))),
        uptime: crash
            .and_then(|c| c.uptime_ms)
            .map(|ms| Second::milli(ms as u32)),
        fault: crash.and_then(|c| c.regs).map(|r| {
            Fault::CortexM(CortexMFault {
                pc: r.pc,
                lr: r.lr,
                xpsr: r.xpsr,
                sp: r.sp,
                cfsr: r.cfsr,
                hfsr: r.hfsr,
                mmfar: r.mmfar,
                bfar: r.bfar,
                exc_return: r.exc_return,
            })
        }),
        stack: MaxLen::new_unchecked(RefVec::Slice {
            slice: crash.map_or(&[][..], |c| c.stack),
        }),
        message: MaxLen::new_unchecked(crash.map_or("", |c| c.message)),
        boots_since_crash: crash.map_or(0, |c| c.boots_since),
    }
}

fn ww_cause(c: Cause) -> ResetCause {
    match c {
        Cause::PowerOn => ResetCause::PowerOn,
        Cause::Brownout => ResetCause::Brownout,
        Cause::Pin => ResetCause::Pin,
        Cause::Watchdog => ResetCause::Watchdog,
        Cause::Software => ResetCause::Software,
        Cause::Lockup => ResetCause::Lockup,
        Cause::Fault => ResetCause::Fault,
        Cause::Panic => ResetCause::Panic,
        Cause::Updated => ResetCause::Updated,
        Cause::Undetermined => ResetCause::Undetermined,
    }
}
