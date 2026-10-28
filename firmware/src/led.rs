//! D14 (PB14, TIM15_CH1 PWM): breathes by default, solid on or off through `led_on` / `led_off`, shown through one
//! indication (`leds` in the API, src/ww/leds.rs): visible or not, a peak brightness, and [`identify`], which
//! blinks it for a while and then returns to the mode. The animation, its math and the task loop are embedded_bedrock's
//! `bedrock_led` (host-tested there); this module sets up the timer. While `crate::sync::SYNC` has shared time (a
//! host steers it, src/ww/clock.rs), breath and blink are aligned to it, so two boards on hosts with NTP breathe in
//! phase; free-running from the last mode change otherwise.
//!
//! The time driver is TIM4 (`time-driver-tim4` in Cargo.toml), named explicitly: `time-driver-any` picks one by
//! embassy's own order and may take TIM15.

pub use bedrock_led::{DEFAULT_PEAK, Mode, look};
use bedrock_led::{Indication, Pwm, SharedTime};
use embassy_stm32::Peri;
use embassy_stm32::gpio::OutputType;
use embassy_stm32::peripherals::{PB14, TIM15};
use embassy_stm32::time::{Hertz, khz};
use embassy_stm32::timer::low_level::CountingMode;
use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm};

/// 1 kHz: no flicker, 64000 steps of duty at 64 MHz
const PWM_FREQ: Hertz = khz(1);

/// What D14 shows after boot unless `device.settings.led_at_boot` says solid on.
pub const DEFAULT_MODE: Mode = look::OK.mode;

static LEDS: Indication<1> = Indication::new();

/// Change D14's mode. Takes effect on the next animation tick; breath and blink restart from dark (or at their
/// place in shared time).
pub fn set(mode: Mode) {
    LEDS.set(0, mode);
}

/// Show or hide D14 (default shown); its animation keeps running hidden.
pub fn set_visible(visible: bool) {
    LEDS.set_visible(0, visible);
}

/// Peak brightness (default [`DEFAULT_PEAK`]), without restarting the animation.
pub fn set_peak(peak: u8) {
    LEDS.set_peak(peak);
}

/// Blink D14 at full brightness for `ms` (restarts a running identify, 0 ends it), then show the mode again.
pub fn identify(ms: u32) {
    LEDS.identify(ms);
}

#[embassy_executor::task]
pub async fn led_task(tim15: Peri<'static, TIM15>, pb14: Peri<'static, PB14>, mode: Mode) {
    let mut pwm = SimplePwm::new(
        tim15,
        Some(PwmPin::new(pb14, OutputType::PushPull)),
        None,
        None,
        None,
        PWM_FREQ,
        CountingMode::EdgeAlignedUp,
    );
    let mut ch = pwm.ch1();
    ch.set_duty_cycle_fully_off();
    ch.enable();
    let mut d14 = Pwm::new(ch);

    LEDS.run_with(
        [mode],
        || {
            crate::sync::SYNC.shared_now().map(|s| SharedTime {
                us: s.us,
                generation: s.generation,
            })
        },
        |leds| bedrock_led::Channel::set_level(&mut d14, leds.level(0)),
    )
    .await
}
