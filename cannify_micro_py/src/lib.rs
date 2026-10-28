//! `cannify_micro` Python module: [Device] wraps `cannify_micro_client::Cannify` with blocking calls on a tokio
//! runtime of its own, so scripts and notebooks need no asyncio. Everything the module does, the library does.

use std::path::PathBuf;
use std::time::Duration;

use cannify_micro_client::{
    BootTarget, Brightness, Cannify, CounterBuffer, Error, Mode, SiliconRevision, USB_ID, Usb,
    api_line, clock_line, firmware_line, node_lines, reset_lines,
};
use pyo3::exceptions::{PyConnectionError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use tokio::runtime::Runtime;

/// Talk to the CANnify Micro firmware over USB.
#[pymodule]
fn cannify_micro(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Device>()?;
    m.add("USB_ID", USB_ID)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}

fn to_py(e: Error) -> PyErr {
    match e {
        Error::Connect(_) => PyConnectionError::new_err(e.to_string()),
        Error::Invalid(_) | Error::Label(_) => PyValueError::new_err(e.to_string()),
        Error::Link { .. }
        | Error::Unimplemented { .. }
        | Error::Device { .. }
        | Error::Elf(_)
        | Error::Decode(_) => PyRuntimeError::new_err(e.to_string()),
    }
}

/// A board reached over USB. Connects when created; use it as a context manager or call `disconnect()` to release
/// the USB device.
///
/// `Device(serial=None, elf=None)`: `serial` picks a board when several are attached (a serial or a part of it, or
/// VID:PID:SERIAL), `elf` (the firmware's ELF) decodes counters and the log (default: the ELF of the board's build
/// id).
#[pyclass]
struct Device {
    runtime: Runtime,
    board: Option<Cannify>,
    elf: Option<PathBuf>,
}

impl Device {
    fn cannify(&self) -> PyResult<&Cannify> {
        self.board
            .as_ref()
            .ok_or_else(|| PyConnectionError::new_err("disconnected"))
    }

    fn block<T>(&self, f: impl Future<Output = cannify_micro_client::Result<T>>) -> PyResult<T> {
        self.runtime.block_on(f).map_err(to_py)
    }
}

#[pymethods]
impl Device {
    #[new]
    #[pyo3(signature = (serial=None, elf=None))]
    fn new(serial: Option<String>, elf: Option<PathBuf>) -> PyResult<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let usb = match serial {
            Some(spec) => Usb::default().select(&spec).map_err(to_py)?,
            None => Usb::default(),
        };
        let board = runtime.block_on(Cannify::connect(&usb)).map_err(to_py)?;
        Ok(Self {
            runtime,
            board: Some(board),
            elf,
        })
    }

    /// API name and version the board reported.
    fn api(&self) -> PyResult<String> {
        Ok(api_line(self.cannify()?.info()))
    }

    /// Switch the user LED (D14) on.
    fn led_on(&self) -> PyResult<()> {
        self.block(self.cannify()?.led_on())
    }

    /// Switch the user LED off.
    fn led_off(&self) -> PyResult<()> {
        self.block(self.cannify()?.led_off())
    }

    /// Blink the LED `count` times, `period_ms` per on-off cycle.
    #[pyo3(signature = (count=3, period_ms=500))]
    fn blink(&self, count: u32, period_ms: u64) -> PyResult<()> {
        self.block(
            self.cannify()?
                .blink(count, Duration::from_millis(period_ms)),
        )
    }

    /// The global indication mode: "normal" shows D14, "essential" and "off" hide it (RAM only, "normal" after a
    /// reset).
    #[getter]
    fn leds_mode(&self) -> PyResult<String> {
        let mode = match self.block(self.cannify()?.leds_mode())? {
            Mode::Normal => "normal",
            Mode::OnlyEssential => "essential",
            Mode::AllOff => "off",
            Mode::Unknown(_) => "unknown",
        };
        Ok(mode.to_string())
    }

    #[setter]
    fn set_leds_mode(&self, mode: &str) -> PyResult<()> {
        let mode = match mode {
            "normal" => Mode::Normal,
            "essential" => Mode::OnlyEssential,
            "off" => Mode::AllOff,
            _ => {
                return Err(PyValueError::new_err(
                    "leds_mode is \"normal\", \"essential\" or \"off\"",
                ));
            }
        };
        self.block(self.cannify()?.set_leds_mode(mode))
    }

    /// D14's peak brightness: "lowest", "low", "normal", "high" or "highest" (RAM only, "normal" after a reset).
    #[getter]
    fn brightness(&self) -> PyResult<String> {
        let level = match self.block(self.cannify()?.brightness())? {
            Brightness::Lowest => "lowest",
            Brightness::Low => "low",
            Brightness::Normal => "normal",
            Brightness::High => "high",
            Brightness::Highest => "highest",
            Brightness::Unknown(_) => "unknown",
        };
        Ok(level.to_string())
    }

    #[setter]
    fn set_brightness(&self, level: &str) -> PyResult<()> {
        let level = match level {
            "lowest" => Brightness::Lowest,
            "low" => Brightness::Low,
            "normal" => Brightness::Normal,
            "high" => Brightness::High,
            "highest" => Brightness::Highest,
            _ => {
                return Err(PyValueError::new_err(
                    "brightness is \"lowest\", \"low\", \"normal\", \"high\" or \"highest\"",
                ));
            }
        };
        self.block(self.cannify()?.set_brightness(level))
    }

    /// Blink D14 at full brightness for `seconds` (up to 600), then show its mode again.
    #[pyo3(signature = (seconds=5.0))]
    fn identify(&self, seconds: f64) -> PyResult<()> {
        let duration = Duration::try_from_secs_f64(seconds)
            .map_err(|e| PyValueError::new_err(format!("identify for {seconds} s: {e}")))?;
        self.block(self.cannify()?.identify(duration))
    }

    /// Wait until the connection has steered the board's shared clock onto this PC's UTC (`Locked`), at most
    /// `timeout` seconds; it takes a few seconds after connecting.
    #[pyo3(signature = (timeout=30.0))]
    fn wait_clock_locked(&self, timeout: f64) -> PyResult<()> {
        let timeout = Duration::try_from_secs_f64(timeout)
            .map_err(|e| PyValueError::new_err(format!("timeout {timeout} s: {e}")))?;
        self.block(self.cannify()?.wait_clock_locked(timeout))?;
        Ok(())
    }

    /// One check of the board's shared time against this PC's UTC: `{"state", "source", "residual_us", "rtt_us",
    /// "slew_us", "error_us", "rate_ppm", "fit_rms_us", "line"}`. `residual_us` is the board's shared time minus
    /// UTC at the middle of the round trip, known to within `rtt_us / 2`; `slew_us` the offset the board still
    /// slews in; `rate_ppm` and `fit_rms_us` come from the connection's fit of the board's clock. A value the board
    /// or the connection can't tell yet is `None`.
    fn clock_check<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let board = self.cannify()?;
        let check = self.block(board.clock_check())?;
        let steering = board.clock_steering().map(|s| s.status());
        let fit = steering.as_ref().and_then(|s| s.fit.as_ref());
        let s = &check.status;
        let d = PyDict::new(py);
        d.set_item("state", format!("{:?}", s.state))?;
        d.set_item("source", s.source.as_ref().map(|src| format!("{src:?}")))?;
        d.set_item("residual_us", check.residual_us)?;
        d.set_item("rtt_us", check.rtt_us)?;
        d.set_item("slew_us", s.slew_us)?;
        d.set_item("error_us", s.error.as_ref().map(|e| e.to_f64() * 1e6))?;
        d.set_item("rate_ppm", fit.map(|f| (f.slope - 1.0) * 1e6))?;
        d.set_item("fit_rms_us", fit.map(|f| f.residual_rms / 1e3))?;
        d.set_item("line", clock_line(&check, steering.as_ref()))?;
        Ok(d)
    }

    /// Name, version, git commit, build time and profile of the running firmware, as text.
    fn firmware(&self) -> PyResult<String> {
        Ok(firmware_line(&self.block(self.cannify()?.firmware())?))
    }

    /// GNU build id of the running firmware, hex.
    fn build_id(&self) -> PyResult<String> {
        Ok(hex(&self.block(self.cannify()?.build_id())?))
    }

    /// The name stored on the board.
    #[getter]
    fn label(&self) -> PyResult<String> {
        self.block(self.cannify()?.label())
    }

    /// Store a name on the board (at most 32 bytes, in flash when the call returns).
    #[setter]
    fn set_label(&self, label: &str) -> PyResult<()> {
        self.block(self.cannify()?.set_label(label))
    }

    /// Why the board last reset, with the crash record after a fault or panic, as text.
    fn last_reset(&self) -> PyResult<String> {
        let board = self.cannify()?;
        let reset = self.block(board.last_reset())?;
        let boots = self.block(board.boot_count())?;
        Ok(reset_lines(&reset, boots).join("\n"))
    }

    /// Boots since the boot counter was cleared.
    fn boot_count(&self) -> PyResult<u32> {
        self.block(self.cannify()?.boot_count())
    }

    /// Forget the crash record and zero the boot counter.
    fn clear_last_reset(&self) -> PyResult<()> {
        self.block(self.cannify()?.clear_last_reset())
    }

    /// Board name, revision and serial number: `{"name", "revision", "serial"}`.
    fn board<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let b = self.block(self.cannify()?.board())?;
        let d = PyDict::new(py);
        d.set_item("name", b.name.as_str())?;
        d.set_item("revision", b.revision.as_str())?;
        d.set_item("serial", b.serial.as_str())?;
        Ok(d)
    }

    /// The board's unique ids, hex: the STM32 96-bit unique id.
    fn uid(&self) -> PyResult<Vec<String>> {
        let ids = self.block(self.cannify()?.uid())?;
        Ok(ids.iter().map(|id| hex(id.value.as_slice())).collect())
    }

    /// The chip on the board: `{"dev_id", "rev_id", "revision", "flash_kib"}`, `revision` is "A", "Z", "Y" or
    /// "unknown".
    fn variant<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let v = self.block(self.cannify()?.variant())?;
        let d = PyDict::new(py);
        d.set_item("dev_id", v.dev_id)?;
        d.set_item("rev_id", v.rev_id)?;
        d.set_item(
            "revision",
            match v.revision {
                SiliconRevision::A => "A",
                SiliconRevision::Z => "Z",
                SiliconRevision::Y => "Y",
                _ => "unknown",
            },
        )?;
        d.set_item("flash_kib", v.flash_kib)?;
        Ok(d)
    }

    /// Every cnt counter, decoded with the firmware's ELF: `{"BUF/group/name": value}`, BUF is RAM or BKP.
    #[pyo3(signature = (force=false))]
    fn counters<'py>(&self, py: Python<'py>, force: bool) -> PyResult<Bound<'py, PyDict>> {
        let report = self.block(self.cannify()?.counters(self.elf.as_deref(), force))?;
        let d = PyDict::new(py);
        for c in &report.counters {
            d.set_item(format!("{}/{}", c.buffer, c.name), c.value)?;
        }
        Ok(d)
    }

    /// Zero the counters of a buffer: "ram" or "bkp".
    fn reset_counters(&self, buffer: &str) -> PyResult<()> {
        let buffer = match buffer {
            "ram" => CounterBuffer::Ram,
            "bkp" => CounterBuffer::Backup,
            _ => return Err(PyValueError::new_err("buffer is \"ram\" or \"bkp\"")),
        };
        self.block(self.cannify()?.reset_counters(buffer))
    }

    /// The defmt log the board keeps (since boot, and from before a crash), decoded: one string per line.
    #[pyo3(signature = (force=false))]
    fn log(&self, force: bool) -> PyResult<Vec<String>> {
        Ok(self
            .block(self.cannify()?.log(self.elf.as_deref(), force))?
            .lines)
    }

    /// The LED after boot, kept in flash: `True` on, `False` off.
    #[getter]
    fn led_at_boot(&self) -> PyResult<bool> {
        self.block(self.cannify()?.led_at_boot())
    }

    #[setter]
    fn set_led_at_boot(&self, on: bool) -> PyResult<()> {
        self.block(self.cannify()?.set_led_at_boot(on))
    }

    /// Every setting back to its default, the label too.
    fn reset_settings(&self) -> PyResult<()> {
        self.block(self.cannify()?.reset_settings())
    }

    /// The boot state: "Confirmed" (the firmware is the only image).
    fn boot_state(&self) -> PyResult<String> {
        Ok(format!("{:?}", self.block(self.cannify()?.boot_state())?))
    }

    /// Reset the board into the firmware, or with `updater=True` into the STM32 ROM bootloader (USB DFU on
    /// PA11/PA12, the board's own USB-C J3). The connection is gone afterwards: the object is disconnected.
    #[pyo3(signature = (updater=false))]
    fn reboot(&mut self, updater: bool) -> PyResult<()> {
        let into = if updater {
            BootTarget::Updater
        } else {
            BootTarget::App
        };
        self.block(self.cannify()?.reboot(into))?;
        self.disconnect()
    }

    /// The update node (product, profile, boot state, last result), as text.
    fn dfu_node(&self) -> PyResult<String> {
        Ok(node_lines(&self.block(self.cannify()?.dfu_node())?).join("\n"))
    }

    /// Everything of the device contract in one text, for a bug report (counters and log decoded with the ELF).
    #[pyo3(signature = (force=false))]
    fn report(&self, force: bool) -> PyResult<String> {
        Ok(self
            .runtime
            .block_on(self.cannify()?.report(self.elf.clone(), force)))
    }

    /// Release the USB device. The object can't be used afterwards.
    fn disconnect(&mut self) -> PyResult<()> {
        if let Some(board) = self.board.take() {
            self.runtime.block_on(board.disconnect());
        }
        Ok(())
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&mut self, _args: &Bound<'_, pyo3::types::PyTuple>) -> PyResult<()> {
        self.disconnect()
    }

    fn __repr__(&self) -> String {
        match &self.board {
            Some(b) => format!("Device({})", api_line(b.info())),
            None => "Device(disconnected)".to_string(),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
