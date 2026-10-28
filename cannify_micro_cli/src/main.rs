//! `cannify`: the command line over `cannify_micro_client`. Argument parsing and printing live here, everything
//! that talks to the board is the library's.
//!
//! Shell completion, with the serials of the boards attached right now: `source <(COMPLETE=bash cannify)` in
//! `~/.bashrc`, `COMPLETE=zsh cannify`, `COMPLETE=fish cannify | source`.

use std::ffi::OsStr;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anstream::println;
use anstyle::{AnsiColor, Color, Style};
use cannify_micro_client::{
    BootTarget, Brightness, Cannify, CounterBuffer, Error, Mode, USB_ID, Usb, api_line, board_line,
    clock_line, firmware_line, node_lines, reset_lines,
};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::engine::{ArgValueCandidates, ArgValueCompleter, PathCompleter, ValueCompleter};
use clap_complete::{CompleteEnv, CompletionCandidate};
use indicatif::{ProgressBar, ProgressStyle};
use nusb::MaybeFuture;

/// `--version`: `0.1.0 (a1b2c3d-dirty, built 23 Oct 2026 10:20 UTC)`, stamped by `build.rs`.
const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("CANNIFY_GIT"),
    ", built ",
    env!("CANNIFY_BUILT"),
    ")"
);

const BOLD: Style = Style::new().bold();
const DIM: Style = Style::new().dimmed();
const GREEN: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)));
const YELLOW: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Yellow)));
const RED: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red)));

/// CANnify Micro over its USB: the LED, the firmware, the board, its label, counters and log, settings, the last
/// reset, reboots into the ROM bootloader.
#[derive(Parser)]
#[command(
    name = "cannify",
    version = VERSION,
    after_help = "Shell completion: source <(COMPLETE=bash cannify), COMPLETE=zsh cannify, COMPLETE=fish cannify | source"
)]
struct Cli {
    /// The board on USB: a serial number (or a part of it), or VID:PID[:SERIAL] (default: the only board with the
    /// firmware's USB id, c0de:cafe, that serves this API)
    #[arg(short, long, env = "CANNIFY_DEVICE", global = true, value_name = "DEVICE", add = ArgValueCandidates::new(usb_serials))]
    device: Option<String>,
    /// The firmware's ELF, to decode counters and the log (default: the ELF of the board's build id, from target/
    /// or what `ww elf add` keeps)
    #[arg(long, env = "CANNIFY_ELF", global = true, value_name = "FILE", add = ArgValueCompleter::new(elf_files))]
    elf: Option<PathBuf>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List the boards on USB with the firmware's USB id
    Devices,
    /// Firmware, API, board, ids, chip, label and last reset of the board
    Info,
    /// The running firmware: name, version, git commit, build time and profile, build id
    Firmware,
    /// Board name, revision and serial number
    Board,
    /// The board's unique ids (the STM32 96-bit unique id)
    Uid,
    /// The chip on the board: device id, silicon revision, flash size
    Variant,
    /// The firmware's cnt counters (RAM and backup registers), decoded with its ELF
    Counters {
        /// Zero the counters of a buffer afterwards
        #[arg(long, value_name = "BUFFER")]
        reset: Option<Buffer>,
        /// Use --elf although its build id differs from the board's
        #[arg(long)]
        force: bool,
    },
    /// The defmt log the board keeps (since boot, and from before a crash), decoded with its ELF
    Log {
        /// Keep printing new lines as they are logged (Ctrl-C stops)
        #[arg(short, long)]
        follow: bool,
        /// Use --elf although its build id differs from the board's
        #[arg(long)]
        force: bool,
    },
    /// Settings kept in the board's flash
    #[command(subcommand)]
    Settings(SettingsCmd),
    /// Boot state, and reboots into the firmware or the STM32 ROM bootloader
    #[command(subcommand)]
    Boot(BootCmd),
    /// The update node: product, profile, boot state, last result
    Dfu,
    /// Everything of the device contract in one text, for a bug report: firmware, board, ids, label, last reset
    /// with the crash's addresses named, counters, the stored log
    Report {
        /// Use --elf although its build id differs from the board's
        #[arg(long)]
        force: bool,
    },
    /// The user LED (D14): solid on, off, blinked from the PC
    #[command(subcommand)]
    Led(LedCmd),
    /// D14's indication: mode, brightness, identify (D14 breathes by default)
    #[command(subcommand)]
    Leds(LedsCmd),
    /// The board's clock against this PC's UTC: the connection steers it, this waits for the lock and prints the
    /// residual (the board's shared time minus UTC) and the round trip it is known to
    Clock {
        /// Keep checking every second for this many seconds
        #[arg(short, long, value_name = "SECONDS", default_value_t = 0)]
        watch: u64,
        /// Give up waiting for the lock after this many seconds
        #[arg(long, value_name = "SECONDS", default_value_t = 30)]
        timeout: u64,
    },
    /// Show the board's label, or store a new one (kept in the board's flash)
    Label {
        /// The new label, at most 32 bytes
        name: Option<String>,
    },
    /// Why the board last reset, with the crash record after a fault or panic
    LastReset {
        /// Forget the record and zero the boot counter afterwards
        #[arg(long)]
        clear: bool,
    },
}

#[derive(Subcommand)]
enum SettingsCmd {
    /// Show every setting
    Show,
    /// The LED after boot: on or off
    LedAtBoot { state: OnOff },
    /// Every setting back to its default, the label too
    Reset,
}

#[derive(Subcommand)]
enum BootCmd {
    /// The boot state (always confirmed: the firmware is the only image)
    State,
    /// Reset the board into the firmware, or into the STM32 ROM bootloader with --updater (USB DFU on PA11/PA12, the board's own USB-C J3;
    /// `just flash` brings the firmware back through the probe)
    Reboot {
        #[arg(long)]
        updater: bool,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum OnOff {
    On,
    Off,
}

/// A counter buffer of the firmware
#[derive(Clone, Copy, clap::ValueEnum)]
enum Buffer {
    /// The RAM counters (zeroed by every reset)
    Ram,
    /// The counters in the backup registers (kept across resets, not power cuts)
    Bkp,
}

#[derive(Subcommand)]
enum LedCmd {
    /// Switch the LED on
    On,
    /// Switch the LED off
    Off,
    /// Blink the LED a few times
    Blink {
        /// How many times
        #[arg(short = 'n', long, default_value_t = 3)]
        count: u32,
        /// One on-off cycle, in milliseconds
        #[arg(long, default_value_t = 500, value_name = "MS")]
        period: u64,
    },
}

#[derive(Subcommand)]
enum LedsCmd {
    /// Show mode and brightness
    Show,
    /// Global mode: normal shows D14, essential and off hide it
    Mode { mode: LedsMode },
    /// Peak brightness
    Brightness { level: Level },
    /// Blink D14 at full brightness to find the board, then back to its mode
    Identify {
        /// How long, in seconds
        #[arg(default_value_t = 5)]
        seconds: u32,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum LedsMode {
    Normal,
    Essential,
    Off,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Level {
    Lowest,
    Low,
    Normal,
    High,
    Highest,
}

/// The USB devices with the firmware's USB id ([USB_ID]); empty on any error.
fn usb_devices() -> Vec<nusb::DeviceInfo> {
    nusb::list_devices()
        .wait()
        .map(|list| {
            list.filter(|d| (d.vendor_id(), d.product_id()) == USB_ID)
                .collect()
        })
        .unwrap_or_default()
}

/// Serials of the boards on USB for `--device`, the product name as help.
fn usb_serials() -> Vec<CompletionCandidate> {
    usb_devices()
        .into_iter()
        .filter_map(|d| {
            let serial = d.serial_number()?.to_string();
            let product = d.product_string().unwrap_or("").to_string();
            Some(CompletionCandidate::new(serial).help(Some(product.into())))
        })
        .collect()
}

/// Firmware ELF files for `--elf`: directories to walk into, files without an extension (cargo's output) or `.elf`.
fn elf_files(current: &OsStr) -> Vec<CompletionCandidate> {
    PathCompleter::file()
        .filter(|path| {
            path.is_dir()
                || path
                    .extension()
                    .is_none_or(|ext| ext.eq_ignore_ascii_case("elf"))
        })
        .complete(current)
}

impl Cli {
    /// The board of `--device` (or `CANNIFY_DEVICE`), else any board with the firmware's USB id.
    fn usb(&self) -> anyhow::Result<Usb> {
        Ok(match self.device.as_deref().filter(|s| !s.is_empty()) {
            Some(spec) => Usb::default().select(spec)?,
            None => Usb::default(),
        })
    }
}

/// Connects, with a spinner on stderr while it takes.
async fn connect(cli: &Cli) -> anyhow::Result<Cannify> {
    let spinner = if std::io::stderr().is_terminal() {
        let s = ProgressBar::new_spinner().with_message("connecting over USB");
        s.set_style(ProgressStyle::with_template("{spinner} {msg}").expect("template"));
        s.enable_steady_tick(Duration::from_millis(80));
        Some(s)
    } else {
        None
    };
    let result = Cannify::connect(&cli.usb()?).await;
    if let Some(s) = spinner {
        s.finish_and_clear();
    }
    Ok(result?)
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    match &cli.command {
        Cmd::Devices => {
            let devices = usb_devices();
            if devices.is_empty() {
                println!(
                    "{YELLOW}no board on USB with id {:04x}:{:04x}{YELLOW:#}",
                    USB_ID.0, USB_ID.1
                );
            }
            for d in devices {
                let serial = d.serial_number().unwrap_or("-");
                println!(
                    "{BOLD}{serial}{BOLD:#}  {}  {DIM}{:04x}:{:04x}{DIM:#}",
                    d.product_string().unwrap_or(""),
                    d.vendor_id(),
                    d.product_id()
                );
            }
        }
        Cmd::Info => {
            let board = connect(&cli).await?;
            let info = board.info();
            println!("{BOLD}API{BOLD:#}        {}", api_line(info));
            let fw = board.firmware().await?;
            println!("{BOLD}firmware{BOLD:#}   {}", firmware_line(&fw));
            match board.build_id().await {
                Ok(id) => println!("{BOLD}build id{BOLD:#}   {}", hex(&id)),
                Err(Error::Unimplemented { .. }) => {}
                Err(e) => return Err(e.into()),
            }
            println!(
                "{BOLD}board{BOLD:#}      {}",
                board_line(&board.board().await?)
            );
            let v = board.variant().await?;
            println!("{BOLD}chip{BOLD:#}       {}", variant_line(&v));
            let label = board.label().await?;
            if label.is_empty() {
                println!("{BOLD}label{BOLD:#}      {DIM}(none){DIM:#}");
            } else {
                println!("{BOLD}label{BOLD:#}      {label}");
            }
            let boots = board.boot_count().await?;
            let reset = board.last_reset().await?;
            for (i, line) in reset_lines(&reset, boots).iter().enumerate() {
                let label = if i == 0 { "last reset" } else { "          " };
                println!("{BOLD}{label}{BOLD:#} {line}");
            }
            board.disconnect().await;
        }
        Cmd::Led(cmd) => {
            let board = connect(&cli).await?;
            match cmd {
                LedCmd::On => board.led_on().await?,
                LedCmd::Off => board.led_off().await?,
                LedCmd::Blink { count, period } => {
                    board.blink(*count, Duration::from_millis(*period)).await?
                }
            }
            board.disconnect().await;
        }
        Cmd::Leds(cmd) => {
            let board = connect(&cli).await?;
            match cmd {
                LedsCmd::Show => {
                    println!("mode        {:?}", board.leds_mode().await?);
                    println!("brightness  {:?}", board.brightness().await?);
                }
                LedsCmd::Mode { mode } => {
                    let mode = match mode {
                        LedsMode::Normal => Mode::Normal,
                        LedsMode::Essential => Mode::OnlyEssential,
                        LedsMode::Off => Mode::AllOff,
                    };
                    board.set_leds_mode(mode).await?;
                }
                LedsCmd::Brightness { level } => {
                    let level = match level {
                        Level::Lowest => Brightness::Lowest,
                        Level::Low => Brightness::Low,
                        Level::Normal => Brightness::Normal,
                        Level::High => Brightness::High,
                        Level::Highest => Brightness::Highest,
                    };
                    board.set_brightness(level).await?;
                }
                LedsCmd::Identify { seconds } => {
                    board
                        .identify(Duration::from_secs(u64::from(*seconds)))
                        .await?
                }
            }
            board.disconnect().await;
        }
        Cmd::Clock { watch, timeout } => {
            let board = connect(&cli).await?;
            let spinner = std::io::stderr().is_terminal().then(|| {
                let s = ProgressBar::new_spinner().with_message("waiting for the clock to lock");
                s.set_style(ProgressStyle::with_template("{spinner} {msg}").expect("template"));
                s.enable_steady_tick(Duration::from_millis(80));
                s
            });
            let locked = board.wait_clock_locked(Duration::from_secs(*timeout)).await;
            if let Some(s) = spinner {
                s.finish_and_clear();
            }
            locked?;
            let steering = board.clock_steering();
            for i in 0..=*watch {
                if i > 0 {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                let check = board.clock_check().await?;
                let status = steering.as_ref().map(|s| s.status());
                println!("{}", clock_line(&check, status.as_ref()));
            }
            board.disconnect().await;
        }
        Cmd::Label { name } => {
            let board = connect(&cli).await?;
            match name {
                Some(name) => {
                    board.set_label(name).await?;
                    println!("{GREEN}label stored{GREEN:#}: {name}");
                }
                None => match board.label().await?.as_str() {
                    "" => println!("{DIM}(none){DIM:#}"),
                    label => println!("{label}"),
                },
            }
            board.disconnect().await;
        }
        Cmd::Firmware => {
            let board = connect(&cli).await?;
            println!("{}", firmware_line(&board.firmware().await?));
            println!("{DIM}build id {}{DIM:#}", hex(&board.build_id().await?));
            board.disconnect().await;
        }
        Cmd::Board => {
            let board = connect(&cli).await?;
            println!("{}", board_line(&board.board().await?));
            board.disconnect().await;
        }
        Cmd::Uid => {
            let board = connect(&cli).await?;
            for id in board.uid().await? {
                println!(
                    "{}  {DIM}{:?}, {:?}{DIM:#}",
                    hex(id.value.as_slice()),
                    id.kind,
                    id.source
                );
            }
            board.disconnect().await;
        }
        Cmd::Variant => {
            let board = connect(&cli).await?;
            println!("{}", variant_line(&board.variant().await?));
            board.disconnect().await;
        }
        Cmd::Counters { reset, force } => {
            let board = connect(&cli).await?;
            let report = board.counters(cli.elf.as_deref(), *force).await?;
            if report.counters.is_empty() {
                println!("{DIM}no counters{DIM:#}");
            }
            for c in &report.counters {
                let style = match c.severity.as_str() {
                    "error" => RED,
                    "warn" => YELLOW,
                    _ => Style::new(),
                };
                println!(
                    "{DIM}{}{DIM:#}  {style}{}{style:#} = {BOLD}{}{BOLD:#} {}",
                    c.buffer, c.name, c.value, c.unit
                );
            }
            println!("{DIM}decoded with {}{DIM:#}", report.elf);
            if let Some(buffer) = reset {
                let (buffer, name) = match buffer {
                    Buffer::Ram => (CounterBuffer::Ram, "RAM"),
                    Buffer::Bkp => (CounterBuffer::Backup, "BKP"),
                };
                board.reset_counters(buffer).await?;
                println!("{GREEN}{name} counters zeroed{GREEN:#}");
            }
            board.disconnect().await;
        }
        Cmd::Log { follow, force } => {
            let board = connect(&cli).await?;
            if *follow {
                // Ctrl-C ends the follow with a clean disconnect; the board keeps the log for the next reader
                tokio::select! {
                    result = board.follow_log(cli.elf.as_deref(), *force, |line| println!("{line}")) => result?,
                    _ = tokio::signal::ctrl_c() => {}
                }
                board.disconnect().await;
            } else {
                let report = board.log(cli.elf.as_deref(), *force).await?;
                for line in &report.lines {
                    println!("{line}");
                }
                board.disconnect().await;
            }
        }
        Cmd::Settings(cmd) => {
            let board = connect(&cli).await?;
            match cmd {
                SettingsCmd::Show => {
                    let on = board.led_at_boot().await?;
                    println!("led-at-boot  {}", if on { "on" } else { "off" });
                }
                SettingsCmd::LedAtBoot { state } => {
                    board.set_led_at_boot(matches!(state, OnOff::On)).await?;
                    println!("{GREEN}stored{GREEN:#}");
                }
                SettingsCmd::Reset => {
                    board.reset_settings().await?;
                    println!("{GREEN}settings back to their defaults{GREEN:#}");
                }
            }
            board.disconnect().await;
        }
        Cmd::Boot(cmd) => {
            let board = connect(&cli).await?;
            match cmd {
                BootCmd::State => println!("{:?}", board.boot_state().await?),
                BootCmd::Reboot { updater } => {
                    let into = if *updater {
                        BootTarget::Updater
                    } else {
                        BootTarget::App
                    };
                    board.reboot(into).await?;
                    if *updater {
                        println!(
                            "{GREEN}rebooting into the STM32 ROM bootloader{GREEN:#} {DIM}(USB DFU on PA11/PA12, the board's own USB-C J3; `just flash` brings the firmware back){DIM:#}"
                        );
                    } else {
                        println!("{GREEN}rebooting{GREEN:#}");
                    }
                }
            }
            board.disconnect().await;
        }
        Cmd::Dfu => {
            let board = connect(&cli).await?;
            for line in node_lines(&board.dfu_node().await?) {
                println!("{line}");
            }
            board.disconnect().await;
        }
        Cmd::Report { force } => {
            let board = connect(&cli).await?;
            println!("{}", board.report(cli.elf.clone(), *force).await);
            board.disconnect().await;
        }
        Cmd::LastReset { clear } => {
            let board = connect(&cli).await?;
            let boots = board.boot_count().await?;
            let reset = board.last_reset().await?;
            for line in reset_lines(&reset, boots) {
                println!("{line}");
            }
            if *clear {
                board.clear_last_reset().await?;
                println!("{GREEN}cleared{GREEN:#}");
            }
            board.disconnect().await;
        }
    }
    Ok(())
}

/// `STM32G0B1 / G0C1 (0x467) rev A (0x1000), 512 KiB flash`.
fn variant_line(v: &cannify_micro_client::Variant) -> String {
    let device = match v.dev_id {
        0x467 => "STM32G0B1 / G0C1",
        _ => "unknown device",
    };
    let revision = match v.revision {
        cannify_micro_client::SiliconRevision::A => "rev A".to_string(),
        cannify_micro_client::SiliconRevision::Z => "rev Z".to_string(),
        cannify_micro_client::SiliconRevision::Y => "rev Y".to_string(),
        _ => "unknown revision".to_string(),
    };
    format!(
        "{device} ({:#05x}) {revision} ({:#06x}), {} KiB flash",
        v.dev_id, v.rev_id, v.flash_kib
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn main() -> ExitCode {
    CompleteEnv::with_factory(Cli::command).complete();
    let cli = Cli::parse();
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            anstream::eprintln!("{RED}error{RED:#}: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(run(cli)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            anstream::eprintln!("{RED}error{RED:#}: {e:#}");
            ExitCode::FAILURE
        }
    }
}
