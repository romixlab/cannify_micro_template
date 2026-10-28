//! `pins` part of the API (`ww_pin_diag::PinDiag`): raw access to every pin the board leaves free, for bring-up and
//! pin tests (`just pins` shows them live). The points are the pins `docs/board/b129a_cannify-pins.md` marks free,
//! named as there. Never the SWD pins (PA13, PA14), USB (PA11, PA12), the LED (PB14), the CAN transceiver (PD1, PB8,
//! PC13), the supply dividers (PB2, PB10), the crystals (PF0, PF1, PC14, PC15) or reset (PF2): they are not in the
//! list.
//!
//! The pins stay in analog mode, the chip's reset state, until a host sets or reads them; `release_all` puts every
//! pin it touched back there, and so does a host that disconnects or goes silent (`ww.rs`). The trait is
//! `#[claim(exclusive)]`: through the hub, one program has the pins at a time.
//!
//! A pin used by the firmware for something else leaves the list in [with_points]: its peripheral is then free for
//! `main.rs` again.

use embassy_stm32::gpio::{Flex, Pull, Speed};
use ww_pin_diag::{Interned, Level, PinDiagError, Point, PointCaps, PointState};

/// Calls `$m!(args.., NAME: CAPS, ...)` with every point, in id order: the one list of the pins `pins` serves.
/// Names are the chip's pin names, as the board description and `ww pins tui` know them.
macro_rules! with_points {
    ($($m:ident)::+!($($args:tt)*)) => {
        $($m)::+! {$($args)*
            PA0: FREE, PA1: FREE, PA2: FREE, PA3: FREE, PA4: FREE, PA5: FREE, PA6: FREE, PA7: FREE, PA8: FREE,
            PA9: FREE, PA10: FREE, PA15: FREE,
            PB0: FREE, PB1: FREE, PB3: FREE, PB4: FREE, PB5: FREE, PB6: FREE, PB7: FREE, PB9: FREE, PB11: FREE,
            PB12: FREE, PB13: FREE, PB15: FREE,
            PD0: FREE, PD2: FREE, PD3: FREE,
        }
    };
}
pub(crate) use with_points;

/// A free pin: an input with either pull, or an output. The G0's output speed sets the edge rate, not the DC
/// drive, so a weak drive is not weaker.
const FREE: PointCaps = PointCaps {
    read: true,
    pull_up: true,
    pull_down: true,
    drive_low: true,
    drive_high: true,
    weak_drive: false,
};

macro_rules! points_table {
    ($($pin:ident: $caps:ident),* $(,)?) => {
        /// Every point, a point's index is its id
        pub const POINTS: &[Point<'static>] = &[$(Point {
            name: Interned::Text(stringify!($pin)),
            group: Interned::Text("mcu"),
            caps: $caps,
        }),*];
    };
}
with_points!(points_table!());

/// Number of points
pub const COUNT: usize = POINTS.len();

/// Takes the pins of [with_points] out of embassy's `Peripherals` (`pins::take!(p)` in main.rs), in id order.
macro_rules! take {
    ($p:ident) => {
        $crate::pins::with_points!(crate::pins::take_flex!($p,))
    };
}
pub(crate) use take;

macro_rules! take_flex {
    ($p:ident, $($pin:ident: $caps:ident),* $(,)?) => {
        [$(embassy_stm32::gpio::Flex::new($p.$pin)),*]
    };
}
pub(crate) use take_flex;

/// The pins and which of them a host touched since the last release.
pub struct Pins {
    pins: [Flex<'static>; COUNT],
    /// Bit per point: not in analog mode any more, `release_all` puts it back
    touched: u64,
}

impl Pins {
    /// The pins of [take], left as they are (analog, the reset state).
    pub fn new(pins: [Flex<'static>; COUNT]) -> Self {
        Pins { pins, touched: 0 }
    }

    pub fn set(&mut self, point: u16, state: PointState) -> Result<(), PinDiagError<'static>> {
        let (pin, caps) = self.point(point)?;
        match state {
            PointState::Input { pull } => {
                let pull = match pull {
                    ww_pin_diag::Pull::None => Pull::None,
                    ww_pin_diag::Pull::Up if caps.pull_up => Pull::Up,
                    ww_pin_diag::Pull::Down if caps.pull_down => Pull::Down,
                    // pulls it doesn't have, the firmware's own (`Custom`) and ones from a newer host
                    _ => return Err(PinDiagError::NotCapable),
                };
                pin.set_as_input(pull);
            }
            // Drive::Weak asks for the weakest drive the pin has: the same one here (see FREE)
            PointState::Output { level, drive: _ } => {
                let level = match level {
                    Level::Low if caps.drive_low => embassy_stm32::gpio::Level::Low,
                    Level::High if caps.drive_high => embassy_stm32::gpio::Level::High,
                    _ => return Err(PinDiagError::NotCapable),
                };
                // level first, then the output: no glitch to the other level on the way
                pin.set_level(level);
                pin.set_as_output(Speed::Low);
            }
            // a state from a newer host
            PointState::Unknown(_) => return Err(PinDiagError::NotCapable),
        }
        self.touched |= 1 << point;
        Ok(())
    }

    pub fn read(&mut self, point: u16) -> Result<Level, PinDiagError<'static>> {
        let untouched = self.touched & (1 << point) == 0;
        let (pin, caps) = self.point(point)?;
        if !caps.read {
            return Err(PinDiagError::NotCapable);
        }
        if untouched {
            // analog mode reads 0: a pin nobody set becomes an input without pulls, until the release
            pin.set_as_input(Pull::None);
        }
        let level = if pin.is_high() {
            Level::High
        } else {
            Level::Low
        };
        self.touched |= 1 << point;
        Ok(level)
    }

    /// Every pin touched since the last release back to analog mode; true when there was one.
    pub fn release_all(&mut self) -> bool {
        let touched = self.touched;
        for (i, pin) in self.pins.iter_mut().enumerate() {
            if touched & (1 << i) != 0 {
                pin.set_as_analog();
            }
        }
        self.touched = 0;
        touched != 0
    }

    fn point(
        &mut self,
        point: u16,
    ) -> Result<(&mut Flex<'static>, PointCaps), PinDiagError<'static>> {
        let i = point as usize;
        match (self.pins.get_mut(i), POINTS.get(i)) {
            (Some(pin), Some(p)) => Ok((pin, p.caps)),
            _ => Err(PinDiagError::NoSuchPoint),
        }
    }
}

// one bit per point in `touched`
const _: () = assert!(COUNT <= 64);
