//! Firmware updates through the STM32 ROM bootloader (`bedrock_rom_boot`): the early boot that runs first thing in
//! main, before clocks and embassy, the reboots `device.boot.reboot` asks for (src/ww/device.rs), and the boot-loop
//! rescue.
//!
//! - `reboot(Updater)` writes the ROM request into the boot word (TAMP_BKP4R, kept out of cnt's backup counters by
//!   memory.x) and resets; [early] finds it and jumps into the ROM bootloader. The ROM listens on USB DFU (PA11 /
//!   PA12, the board's own USB-C J3) and on its other interfaces (AN2606); the probe reaches the chip whatever
//!   runs, so `just flash` works from the ROM too.
//! - A boot loop: every boot counts in the boot word, [healthy_task] clears the count once the firmware has run
//!   [HEALTHY_AFTER] (the USB host configuring the board would call it healthy earlier,
//!   `bedrock_rom_boot::usb_configured`; not wired in yet). After `bedrock_rom_boot::BOOT_LOOP_LIMIT` boots without
//!   that, [early] says `Start::Rescue` and main resets into the ROM bootloader; the next firmware reports it in
//!   `device.dfu.node` as `Outcome::Recovery`. Flashing four times within 10 s of each other counts the same way:
//!   the fourth boot lands in the ROM, the next flash comes back.

use bedrock_rom_boot::{Early, Start};
use defmt::{error, info, unwrap, warn};
use embassy_executor::Spawner;
use embassy_time::Duration;

/// Up this long without a crash: the boot is healthy, the boot-loop count starts over.
pub const HEALTHY_AFTER: Duration = Duration::from_secs(10);

/// First thing in main, before embassy's init: resets once when the ROM bootloader jumped here, goes to the ROM
/// bootloader when asked to (never returns then), counts the boot otherwise.
pub fn early() -> Early {
    bedrock_rom_boot::early()
}

/// Logs what [early] found; once RTT is up.
pub fn log(early: &Early) {
    if let Some(entered) = early.jumped_before {
        info!(
            "boot: reset once after the ROM bootloader's jump ({})",
            entered
        );
    }
    if early.jumped_twice {
        cnt::bkp_cnt!(rom_jumped_twice: u32, warn);
        warn!("boot: entered by a jump twice in a row, going on");
    }
    if early.found.fell_back {
        warn!("boot: the ROM bootloader took over after a boot loop");
    }
    info!(
        "boot: {}, {} boots not yet healthy",
        early.start, early.word.unhealthy_boots
    );
}

/// After embassy's init: the boot word written again, the rescue when [early] said so, the boot's tasks.
pub fn after_init(spawner: Spawner, early: &Early) {
    bedrock_rom_boot::after_init();
    if early.start == Start::Rescue {
        // nothing to bring up for the ROM's USB: J3 is wired straight to PA11 / PA12
        error!("boot: boot loop, resetting into the ROM bootloader");
        bedrock_rom_boot::rescue();
    }
    spawner.spawn(unwrap!(reboot_task()));
    spawner.spawn(unwrap!(healthy_task()));
}

/// Resets when `device.boot.reboot` asked for it, once its reply has left.
#[embassy_executor::task]
async fn reboot_task() {
    bedrock_rom_boot::reboot_when_asked().await
}

/// Marks the boot healthy after [HEALTHY_AFTER].
#[embassy_executor::task]
async fn healthy_task() {
    let why = bedrock_rom_boot::healthy_watch(HEALTHY_AFTER).await;
    info!("boot: healthy ({})", why);
}
