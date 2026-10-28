//! `device`: the device contract (`cannify_micro_api::Device`), and the values of its `const` items.

use super::ServerState;
use cannify_micro_api::{SiliconRevision, Variant};
use core::sync::atomic::AtomicU32;
use embassy_stm32::pac;
use wire_weaver::prelude::*;
use ww_board_info::{BoardSummary, RevisionSource};
use ww_counters::{
    BufferInfo, CounterBuffer, CountersError, CountersInfo, read_words, reset_words,
};
use ww_dfu::{BootState, BootTarget, Error as DfuError, NodeInfo};
use ww_error::CommonError;
use ww_firmware_info::{BUILD_ID_MAX_LEN, FirmwareSummary, ResetRecord};
use ww_log::{DefmtInfo, LogError};
use ww_pin_diag::Point;
use ww_settings::shrink_wrap::ArrayString;
use ww_uid::UniqueId;
use ww_user_info::{MAX_NAME_LEN, UserInfoCaps, UserInfoError};

/// Product name of `device.board` and `device.dfu.node`.
const PRODUCT: &str = "B129A CANnify Micro HV Pico-Lock";

// ww_firmware_info::LastReset at device.last_reset: why the device last reset, and the crash record after a fault or
// panic.
impl ServerState {
    pub(super) fn device_last_reset_last_reset(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<ResetRecord<'_>> {
        Ready(crate::last_reset::record(&self.last_reset))
    }

    pub(super) fn device_last_reset_boot_count(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<u32> {
        Ready(self.last_reset.boot_count())
    }

    pub(super) fn device_last_reset_clear(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<()> {
        bedrock_crash::clear(&mut self.last_reset);
        Ready(())
    }
}

// ww_user_info::UserInfo at device.label: the name a user gives this unit. A write is in flash before the call
// answers, and the flash is written only when the name changed.
impl ServerState {
    pub(super) async fn set_device_label_name(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: MaxLen<&'_ str, MAX_NAME_LEN>,
    ) -> SetResult<UserInfoError<'_>> {
        let Ok(label) = ArrayString::try_from(*value) else {
            return SetError(UserInfoError::TooLong {
                max: MAX_NAME_LEN as u16,
            });
        };
        self.settings.set(|s| s.label = label);
        match self.persist_settings().await {
            Ok(()) => Set,
            Err(e) => SetError(UserInfoError::Common(e)),
        }
    }

    pub(super) async fn get_device_label_name(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<MaxLen<&'_ str, MAX_NAME_LEN>, UserInfoError<'_>> {
        Value(MaxLen::new_unchecked(self.settings.get().label.as_str()))
    }
}

// ww_counters::Counters at device.counters: the cnt counters in RAM and in the backup registers.
impl ServerState {
    pub(super) fn device_counters_read(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        buffer: CounterBuffer,
        offset: u16,
    ) -> RpcResult<Result<MaxLen<RefVec<'_, UVlq32>, { ww_counters::MAX_WORDS }>, CountersError<'_>>>
    {
        let (words, used) = counter_buffer(buffer);
        Ready(read_words(words, used, offset, &mut self.counters_page))
    }

    pub(super) fn device_counters_reset(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        buffer: CounterBuffer,
    ) -> RpcResult<Result<(), CountersError<'_>>> {
        reset_words(counter_buffer(buffer).0);
        Ready(Ok(()))
    }
}

/// A `cnt` buffer and its used words.
fn counter_buffer(buffer: CounterBuffer) -> (&'static [AtomicU32], usize) {
    match buffer {
        CounterBuffer::Ram => (cnt::counters_ram_buffer(), cnt::ram_used_words()),
        CounterBuffer::Backup => (cnt::counters_bkp_buffer(), cnt::bkp_used_words()),
    }
}

// ww_log::LogDefmt at device.log: the defmt log (src/log.rs). Its session starts at boot, so a host that resumes it
// gets the log from before it connected (and from before a crash); opening it anew drops what is kept.
impl ServerState {
    pub(super) fn device_log_frames(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<Result<(), LogError<'_>>> {
        Ready(Ok(()))
    }

    pub(super) fn close_device_log_frames(&mut self, _cx: &mut Context<'_, impl EventOut>) {}

    pub(super) fn stream_device_log_frames(&mut self) -> &mut crate::log::Stream {
        &mut self.log_frames
    }
}

// device.variant: the chip, read from the chip itself.
impl ServerState {
    pub(super) fn get_device_variant(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<Variant, ()> {
        Value(variant())
    }
}

/// DBGMCU_IDCODE and the FLASH_SIZE word (RM0444 40.4.1, 1.5).
fn variant() -> Variant {
    // the DBGMCU registers need their APB clock (RCC_APBENR1.DBGEN)
    pac::RCC.apbenr1().modify(|w| w.set_dbgen(true));
    let idcode = pac::DBGMCU.idcode().read();
    const FLASH_SIZE: *const u16 = 0x1FFF_75E0 as *const u16;
    // SAFETY: the engineering bytes, readable at any time
    let flash_kib = unsafe { FLASH_SIZE.read_volatile() };
    Variant {
        // DEV_ID is bits 11:0; the PAC's field also takes the reserved 15:12, which read 0x6 on a G0B1
        dev_id: idcode.dev_id() & 0xFFF,
        rev_id: idcode.rev_id(),
        revision: SiliconRevision::from_rev_id(idcode.rev_id()),
        flash_kib,
    }
}

// cannify_micro_api::Settings at device.settings: kept in flash, a write is there before the call answers.
impl ServerState {
    pub(super) async fn set_device_settings_led_at_boot(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        value: bool,
    ) -> SetResult<CommonError<'_>> {
        self.settings.set(|s| s.led_at_boot = value);
        match self.persist_settings().await {
            Ok(()) => Set,
            Err(e) => SetError(e),
        }
    }

    pub(super) async fn get_device_settings_led_at_boot(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> GetResult<bool, CommonError<'_>> {
        Value(self.settings.get().led_at_boot)
    }

    pub(super) async fn device_settings_reset(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<Result<(), CommonError<'_>>> {
        self.settings.reset_to_defaults();
        Ready(self.persist_settings().await)
    }

    /// Writes the settings before answering, so `Ok` means they survive a power cut.
    async fn persist_settings(&mut self) -> Result<(), CommonError<'static>> {
        self.settings.persist().await.map_err(|_e| {
            defmt::error!("settings not saved: {}", defmt::Debug2Format(&_e));
            CommonError::Fault { code: 1 }
        })
    }
}

// ww_dfu::BootControl at device.boot and ww_dfu::UpdateNode at device.dfu.node: the firmware in place, updated by
// the STM32 ROM bootloader (src/boot.rs).
impl ServerState {
    pub(super) fn device_boot_boot_state(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<BootState> {
        Ready(bedrock_rom_boot::dfu::boot_state())
    }

    pub(super) fn device_boot_confirm(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<Result<(), DfuError>> {
        Ready(bedrock_rom_boot::dfu::confirm())
    }

    pub(super) fn device_boot_revert(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<Result<(), DfuError>> {
        Ready(bedrock_rom_boot::dfu::revert())
    }

    /// The reply goes out first: the reset follows `bedrock_rom_boot::REPLY_MS` later (src/boot.rs).
    pub(super) fn device_boot_reboot(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        into: BootTarget,
    ) -> RpcResult<Result<(), DfuError>> {
        defmt::info!("reboot into {}", into);
        bedrock_rom_boot::request_reboot(bedrock_rom_boot::dfu::reboot_target(into));
        Ready(Ok(()))
    }

    pub(super) fn device_dfu_node_node(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<NodeInfo<'_>> {
        Ready(bedrock_rom_boot::dfu::node(
            PRODUCT,
            bedrock_rom_boot::fell_back(),
        ))
    }
}

// Values of the API's `const` items (device and leds), served without a handler and read once per connection by a
// host.
impl super::server_impl::CannifyMicroApiConsts for ServerState {
    // name, version, git commit, build time and profile, from build.rs (ww_firmware_info::build::emit())
    const DEVICE_FIRMWARE_FIRMWARE: FirmwareSummary<'static> =
        ww_firmware_info::firmware_summary!();

    // GNU build id, kept in flash by memory.x: a host finds this firmware's ELF by it
    fn device_firmware_build_id(&self) -> MaxLen<RefVec<'_, u8>, BUILD_ID_MAX_LEN> {
        MaxLen::new_unchecked(RefVec::new_bytes(ww_firmware_info::build_id()))
    }

    // the board revision is the one docs/board describes, the firmware cannot read it; the serial is the chip's
    // unique id
    fn device_board_board(&self) -> BoardSummary<'_> {
        BoardSummary {
            name: MaxLen::new_unchecked(PRODUCT),
            revision: MaxLen::new_unchecked("A"),
            revision_source: RevisionSource::Assumed,
            variant: MaxLen::new_unchecked(""),
            bom_variant: MaxLen::new_unchecked(""),
            serial: MaxLen::new_unchecked(embassy_stm32::uid::uid_hex()),
            build_date: None,
        }
    }

    fn device_uid_ids(&self) -> MaxLen<RefVec<'_, UniqueId<'_>>, 4> {
        MaxLen::new_unchecked(RefVec::Slice { slice: &self.uid })
    }

    // kept in the settings store, no room for notes (`omit` in ww.rs)
    const DEVICE_LABEL_CAPABILITIES: UserInfoCaps = UserInfoCaps {
        name_max: MAX_NAME_LEN as u16,
        notes_max: 0,
        persistent: true,
    };

    fn device_counters_info(&self) -> CountersInfo<'_> {
        let bkp = cnt::counters_bkp_buffer();
        CountersInfo {
            format: cnt::SIGNATURE_VERSION,
            ram: BufferInfo::new(cnt::ram_used_words(), cnt::counters_ram_buffer()),
            backup: (!bkp.is_empty()).then(|| BufferInfo::new(cnt::bkp_used_words(), bkp)),
            build_id: MaxLen::new_unchecked(RefVec::new_bytes(ww_firmware_info::build_id())),
        }
    }

    // the encoding defmt is built with, found at run time; a host decodes the frames with the ELF of this build id
    fn device_log_defmt(&self) -> DefmtInfo<'_> {
        DefmtInfo {
            encoding: crate::log::encoding(),
            build_id: MaxLen::new_unchecked(RefVec::new_bytes(ww_firmware_info::build_id())),
        }
    }

    // `pins` (beside the device contract, ww.rs): the free pins, from the table in src/pins.rs
    fn pins_points(&self) -> RefVec<'_, Point<'_>> {
        RefVec::Slice {
            slice: crate::pins::POINTS,
        }
    }

    // ww_indication::Indication at leds: its one LED (src/ww/leds.rs)
    const LEDS_NAMES: RefVec<'static, &'static str> = super::leds::NAMES;
}
