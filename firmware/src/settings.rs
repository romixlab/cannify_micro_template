//! Settings kept in flash: [DeviceSettings] in the `SETTINGS` region of `memory.x` (8K, the end of bank 2),
//! through `ww_settings`: a record of the fields that differ from their default, power-cut safe at any point of a
//! write or an erase, readable by older and newer firmware.
//!
//! Adding a setting: a field with the next free `slot` and a `default`. A slot belongs to its field for good: a
//! removed field's slot goes into `#[settings(retired(..))]`, never to another field. Changing a default changes
//! the value of every device that never set the field. Field types: numbers, `bool`, `ArrayString` / `ArrayVec`,
//! enums and structs with `#[derive_shrink_wrap]` (an API type can be a setting as it is).
//! Rules and sizes: https://github.com/vhrdtech/wire_weaver/blob/master/docs/api/settings.md
//!
//! Written when it settles: [SettingsStore::set] changes RAM, the record is written 1 s after the last change (10 s
//! at most), by whoever calls [SettingsStore::persist_if_due] (the server task in ww.rs). Where an `Ok` must mean
//! "survives a power cut", call [SettingsStore::persist] before answering (the `label` handler in ww.rs).
//!
//! The flash has ECC: a word whose write a power cut interrupted raises an NMI when read. `EccSafeFlash` reads it
//! as zeros instead (the store skips it like any cut write), with `ww_ecc_flash::on_nmi()` in the NMI handler
//! (main.rs). Without it such a word would fault on every boot.
//!
//! Code running from bank 1 keeps running while bank 2 erases; code placed in bank 2 (an image above half the
//! flash) stalls like on a single-bank part.

use defmt::{Debug2Format, error, info};
use embassy_stm32::Peri;
use embassy_stm32::flash::{Blocking, FLASH_BASE, Flash};
use embassy_stm32::peripherals::FLASH;
use embassy_time::Instant;
use ww_ecc_flash::EccSafeFlash;
use ww_settings::shrink_wrap::ArrayString;
use ww_settings::{Debounce, Key, Persisted, Settings, Store};

/// This firmware's settings (key `Key::USER`: what users change).
#[derive(ww_settings::Settings, Clone, Debug, PartialEq)]
pub struct DeviceSettings {
    /// Name the user gives this unit, served as `label` (`ww_user_info::UserInfo`)
    #[setting(slot = 0, default = ArrayString::new())]
    pub label: ArrayString<32>,
    /// The LED after boot, served as `device.settings.led_at_boot`
    #[setting(slot = 1, default = false)]
    pub led_at_boot: bool,
}

/// embassy-stm32's flash, read through `EccSafeFlash` (a word with an ECC error reads as zeros)
pub type SettingsFlash = EccSafeFlash<Flash<'static, Blocking>>;
type FlashError = ww_ecc_flash::Error<embassy_stm32::flash::Error>;
/// What can go wrong when writing the settings
pub type Error = ww_settings::Error<FlashError>;

unsafe extern "C" {
    // memory.x: offsets of SETTINGS from the start of the flash
    static __settings_start: u8;
    static __settings_end: u8;
}

/// [DeviceSettings] in RAM, and the flash store they are written to.
pub struct SettingsStore {
    store: Store<SettingsFlash, 256>,
    settings: Settings<DeviceSettings, 64>,
    /// After a failed write: no new try before this
    retry_at: Instant,
}

/// Wait after a failed write before [SettingsStore::persist_if_due] tries again
const RETRY_MS: u64 = 10_000;

impl SettingsStore {
    /// Reads the settings from flash; any field not stored is at its default. Never writes the flash.
    pub async fn load(flash: Peri<'static, FLASH>) -> Self {
        let flash = Flash::new_blocking(flash);
        // SAFETY: offsets of embassy's `Flash` start at FLASH_BASE
        let flash = unsafe { EccSafeFlash::new(flash, FLASH_BASE as u32) };
        let range = (&raw const __settings_start) as u32..(&raw const __settings_end) as u32;
        let store =
            Store::new(flash, range).expect("SETTINGS in memory.x: page aligned, at least 2 pages");
        let mut this = SettingsStore {
            store,
            settings: Settings::new(Key::USER, Debounce::default()),
            retry_at: Instant::MIN,
        };
        match this.settings.load(&mut this.store).await {
            Ok(report) => info!("settings: {}", report),
            Err(e) => error!("settings not read, defaults: {}", Debug2Format(&e)),
        }
        this
    }

    pub fn get(&self) -> &DeviceSettings {
        self.settings.get()
    }

    /// Changes the settings in RAM; written by [Self::persist_if_due] once they settle. `false`: nothing changed.
    pub fn set(&mut self, f: impl FnOnce(&mut DeviceSettings)) -> bool {
        self.settings.set(now_ms(), f)
    }

    /// Every setting back to its default in RAM; written by [Self::persist_if_due] once they settle, or by
    /// [Self::persist].
    pub fn reset_to_defaults(&mut self) {
        self.settings.reset_to_defaults(now_ms());
    }

    /// Writes the settings now (if they differ from what is stored): `Ok` means they survive a power cut.
    pub async fn persist(&mut self) -> Result<(), Error> {
        if let Persisted::Written(_) = self.settings.persist(&mut self.store).await? {
            self.erase_ahead().await;
        }
        Ok(())
    }

    /// When [Self::persist_if_due] has something to write, `Instant::MAX` when nothing changed.
    pub fn due_at(&self) -> Instant {
        self.settings.due_at().map_or(Instant::MAX, |at| {
            Instant::from_millis(at).max(self.retry_at)
        })
    }

    /// Writes the settings if they changed and settled ([Self::due_at]).
    pub async fn persist_if_due(&mut self) {
        if Instant::now() < self.retry_at {
            return;
        }
        match self
            .settings
            .persist_if_due(&mut self.store, now_ms())
            .await
        {
            Ok(Some(Persisted::Written(_))) => self.erase_ahead().await,
            Ok(_) => {}
            Err(_e) => {
                // kept in RAM, tried again in RETRY_MS
                self.retry_at = Instant::now() + embassy_time::Duration::from_millis(RETRY_MS);
                error!("settings not written: {}", Debug2Format(&_e));
            }
        }
    }

    /// Erases the page the next compaction moves to, if it needs it, so a later write doesn't erase.
    async fn erase_ahead(&mut self) {
        if let Err(e) = self.store.maintain().await {
            error!("settings erase ahead failed: {}", Debug2Format(&e));
        }
    }
}

fn now_ms() -> u64 {
    Instant::now().as_millis()
}
