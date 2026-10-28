//! `leds`: D14 as one `ww_indication::Indication`, shown through src/led.rs. RAM only: mode and brightness are back
//! to `Normal` after a reset. The peaks and the identify bookkeeping are `bedrock_led::indication`, shared with the
//! other bedrock boards.

use super::ServerState;
use crate::led::{self, DEFAULT_PEAK};
use bedrock_led::indication::{Identify, PEAKS};
use cnt::cnt;
use embassy_time::Instant;
use wire_weaver::prelude::*;
use ww_error::CommonError;
use ww_indication::{Brightness, Mode};
use ww_si::ww_numeric::NumericValue;
use ww_si::{Prefix, Second};

/// `leds.NAMES`
pub(super) const NAMES: RefVec<'static, &'static str> = RefVec::Slice { slice: &["D14"] };

/// What the API last set, src/led.rs only takes it.
pub(super) struct LedsState {
    mode: Mode,
    brightness: Brightness,
    /// The running identify, for `Busy`.
    identify: Identify<Instant>,
}

impl LedsState {
    pub(super) const fn new() -> Self {
        LedsState {
            mode: Mode::Normal,
            brightness: Brightness::Normal,
            identify: Identify::new(),
        }
    }
}

// ww_indication::Indication at leds
impl ServerState {
    /// `OnlyEssential` hides D14 too: it is the user LED, nothing essential shows on it (the board has no power
    /// LED, D14 is its only one).
    pub(super) fn set_leds_mode(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: Mode,
    ) -> SetResult<()> {
        let visible = match value {
            Mode::AllOff | Mode::OnlyEssential => false,
            Mode::Normal => true,
            // a mode from a newer host: what it means is unknown, show the LED
            _ => {
                cnt!(leds_mode_unknown: u32);
                true
            }
        };
        led::set_visible(visible);
        self.leds.mode = value;
        Set
    }

    pub(super) fn get_leds_mode(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<Mode, ()> {
        Value(self.leds.mode)
    }

    pub(super) fn set_leds_brightness(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: Brightness,
    ) -> SetResult<()> {
        let peak = PEAKS.peak(value).unwrap_or_else(|| {
            cnt!(leds_brightness_unknown: u32);
            DEFAULT_PEAK
        });
        led::set_peak(peak);
        self.leds.brightness = value;
        Set
    }

    pub(super) fn get_leds_brightness(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<Brightness, ()> {
        Value(self.leds.brightness)
    }

    /// D14 blinks at full brightness, whatever the mode; `Busy` while one runs.
    pub(super) fn leds_identify(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        duration: Second,
    ) -> RpcResult<Result<(), CommonError<'_>>> {
        let now = Instant::now();
        if self.leds.identify.busy(now) {
            return Ready(Err(CommonError::Busy));
        }
        let Some(ms) = to_ms(&duration) else {
            return Ready(Err(CommonError::InvalidArgument {
                arg: 0,
                reason: MaxLen::new_unchecked("not an integer number of s, ms or us"),
            }));
        };
        led::identify(self.leds.identify.start(now, ms));
        Ready(Ok(()))
    }
}

/// A duration in milliseconds, for the integer encodings a host sends. No float or 64-bit math (M0+).
fn to_ms(d: &Second) -> Option<u32> {
    let v = match d.value {
        NumericValue::U8(v) => v as u32,
        NumericValue::U32(v) => v,
        _ => return None,
    };
    match d.prefix {
        Prefix::Unit => Some(v.saturating_mul(1000)),
        Prefix::Milli => Some(v),
        Prefix::Micro => Some(v / 1000),
        _ => None,
    }
}
