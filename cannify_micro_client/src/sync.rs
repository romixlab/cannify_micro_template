//! `leds` and `clock`: D14's indication (mode, brightness, identify) and the board's clock. The connection steers the
//! board's shared clock onto this PC's UTC by itself (WireWeaver's clock sync, on by default, `WW_CLOCK_SYNC=off`
//! turns it off), so D14 breathes in phase with other boards steered by PCs on NTP. [Cannify::clock_check] measures
//! how well it follows.

use std::time::Duration;

use wire_weaver_client::clock_sync::{ClockSteering, HostClock, Phase, Status};
use ww_si::Second;

use crate::{Cannify, Error, Result, call, call_fallible, read};

pub use ww_indication::{Brightness, Mode};
pub use ww_timebase::{SyncState, SyncStatusOwned};

/// One check of the board's shared time against this PC's UTC, see [Cannify::clock_check].
#[derive(Clone, Debug)]
pub struct ClockCheck {
    /// The board's own sync state, source, offset and error bound.
    pub status: SyncStatusOwned,
    /// The board's shared time minus this PC's UTC at the middle of the round trip, in µs; `None` while the board's
    /// clock runs free.
    pub residual_us: Option<f64>,
    /// Round trip of the `shared_now` call, in µs: the residual is known to within half of it.
    pub rtt_us: f64,
}

impl Cannify {
    /// The global indication mode: `Normal` shows D14, `OnlyEssential` and `AllOff` hide it (RAM only, `Normal` after
    /// a reset).
    pub async fn leds_mode(&self) -> Result<Mode> {
        read("Indication::mode", self.raw.leds().read_mode()).await
    }

    pub async fn set_leds_mode(&self, mode: Mode) -> Result<()> {
        let item = "Indication::mode";
        let w = self.raw.leds().write_mode(mode);
        w.write()
            .await
            .map_err(|source| Error::Link { item, source })
    }

    /// D14's peak brightness (RAM only, `Normal` after a reset).
    pub async fn brightness(&self) -> Result<Brightness> {
        read("Indication::brightness", self.raw.leds().read_brightness()).await
    }

    pub async fn set_brightness(&self, brightness: Brightness) -> Result<()> {
        let item = "Indication::brightness";
        let w = self.raw.leds().write_brightness(brightness);
        w.write()
            .await
            .map_err(|source| Error::Link { item, source })
    }

    /// Blinks D14 at full brightness for `duration` (up to 10 minutes), then shows its mode again; an error while
    /// one runs (`Busy`).
    pub async fn identify(&self, duration: Duration) -> Result<()> {
        let ms = u32::try_from(duration.as_millis())
            .map_err(|_| Error::Invalid(format!("identify for {duration:?}: too long")))?;
        call_fallible(
            "Indication::identify",
            self.raw.leds().identify(Second::milli(ms)),
        )
        .await
    }

    /// The connection's own steering of the board's clock: its phase and status (the fit of the board's clock, the
    /// residual of its last check). `None` when it is off (`WW_CLOCK_SYNC=off`).
    pub fn clock_steering(&self) -> Option<ClockSteering> {
        self.raw.cmd.clock_sync()
    }

    /// Waits until the board says its shared clock is `Locked` onto this PC's UTC (or `timeout`); returns the last
    /// steering status. Steering takes a few seconds after connecting: the fit needs samples first.
    pub async fn wait_clock_locked(&self, timeout: Duration) -> Result<Status> {
        let steering = self.clock_steering().ok_or_else(|| {
            Error::Invalid("clock steering is off (WW_CLOCK_SYNC=off)".to_string())
        })?;
        let mut watch = steering.watch();
        let locked = tokio::time::timeout(timeout, async {
            loop {
                if let Phase::Failed(why) = steering.phase() {
                    return Err(Error::Invalid(format!("clock steering failed: {why}")));
                }
                if matches!(steering.phase(), Phase::Unsupported) {
                    return Err(Error::Unimplemented {
                        item: "Clock::steer",
                    });
                }
                let locked = watch
                    .borrow()
                    .device
                    .as_ref()
                    .is_some_and(|d| d.state == SyncState::Locked);
                if locked {
                    return Ok(());
                }
                if watch.changed().await.is_err() {
                    return Err(Error::Invalid("clock steering stopped".to_string()));
                }
            }
        })
        .await;
        match locked {
            Ok(Ok(())) => Ok(steering.status()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(Error::Invalid(format!(
                "the board's clock did not lock within {timeout:?}: {:?}",
                steering.status().device.map(|d| d.state)
            ))),
        }
    }

    /// Reads the board's sync status, then its shared time once, against this PC's UTC at the middle of that round
    /// trip: what the sync achieves, to within half the round trip (RTT polls the probe, so round trips are
    /// milliseconds).
    pub async fn clock_check(&self) -> Result<ClockCheck> {
        let status = call("Clock::sync_status", self.raw.clock().sync_status()).await?;
        let host = HostClock::new();
        let utc_offset = host.utc_offset();
        let before = host.monotonic_ns();
        let shared = call("Clock::shared_now", self.raw.clock().shared_now()).await?;
        let after = host.monotonic_ns();
        let mid_utc_ns = (before + (after - before) / 2) as i128 + utc_offset;
        Ok(ClockCheck {
            status,
            residual_us: shared.map(|us| (us as i128 * 1000 - mid_utc_ns) as f64 / 1000.0),
            rtt_us: (after - before) as f64 / 1000.0,
        })
    }
}

/// One line for a clock check and the connection's steering status:
/// `Locked (Host), residual +0.21 ms ± 1.10 ms, slew 0 µs, error ≤ 2.00 ms, rate -312.4 ppm, fit rms 0.35 ms`.
pub fn clock_line(check: &ClockCheck, steering: Option<&Status>) -> String {
    let s = &check.status;
    let mut line = format!("{:?}", s.state);
    if let Some(source) = &s.source {
        line.push_str(&format!(" ({source:?})"));
    }
    match check.residual_us {
        Some(r) => line.push_str(&format!(
            ", residual {:+.2} ms ± {:.2} ms",
            r / 1000.0,
            check.rtt_us / 2000.0
        )),
        None => line.push_str(", no shared time"),
    }
    if let Some(slew) = s.slew_us {
        line.push_str(&format!(", slew {slew} µs"));
    }
    if let Some(error) = &s.error {
        line.push_str(&format!(", error ≤ {:.2} ms", error.to_f64() * 1e3));
    }
    if let Some(fit) = steering.and_then(|st| st.fit.as_ref()) {
        line.push_str(&format!(
            ", rate {:+.1} ppm, fit rms {:.2} ms",
            (fit.slope - 1.0) * 1e6,
            fit.residual_rms / 1e6
        ));
    }
    line
}
