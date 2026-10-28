//! Low level init that HALs do not cover. Generated from stm32-data; verify against the reference manual.

#[allow(unused_imports)]
use embassy_stm32::pac;

pub(crate) fn init() {
    enable_backup_registers();
}

/// Enable write access to TAMP backup registers used by `bkp_cnt!` counters.
/// Backup registers are only accessible with DBP set and (on most families) the RTC APB clock enabled.
/// NOTE: TAMP/RTC backup registers are reset by a backup domain reset, so do not call reset_backup_domain().
pub(crate) fn enable_backup_registers() {
    let rcc = pac::RCC;
    let pwr = pac::PWR;
    rcc.apbenr1().modify(|w| w.set_pwren(true));
    let _ = rcc.apbenr1().read(); // make sure the enable went through
    pwr.cr1().modify(|w| w.set_dbp(true));
    rcc.apbenr1().modify(|w| w.set_rtcapben(true));
    // TODO: verify on your part whether RTCEN (with a clock source) is required for backup register writes
    // rcc.bdcr().modify(|w| { w.set_rtcsel(pac::rcc::vals::Rtcsel::LSI); w.set_rtcen(true); });
}
