//! The shared clock: a host's steer points discipline the local clock (embassy-time on TIM4) onto UTC, so D14
//! (src/led.rs) breathes in phase with other boards. Served as `clock` (`ww_timebase::Clock`) by src/ww/clock.rs; this
//! module only holds the static, as src/led.rs holds the indication.
//!
//! No sync line and no oscillator backend: the 64 MHz comes from the 12 MHz crystal Y2 through the PLL, the shared clock is
//! corrected in software only (bedrock_sync slews it).

/// The board's shared clock, read by the LED task every tick and steered from `clock.steer` (src/ww/clock.rs).
/// bedrock_sync's defaults: locked within 2 ms, sized for a full-speed USB board.
pub static SYNC: bedrock_sync::Sync = bedrock_sync::Sync::new(bedrock_sync::Config::DEFAULT);
