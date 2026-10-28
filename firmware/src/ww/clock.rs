//! `clock`: `ww_timebase::Clock`, served through embedded_bedrock's `bedrock_sync`: the host's `steer` points
//! discipline the local clock onto UTC (`crate::sync::SYNC`), which src/led.rs aligns D14's breath to. A host's
//! WireWeaver client steers on its own, about once a second while connected. No sync line (`ClockServer::NO_LINE`:
//! `configure_sync` answers `Unsupported`, `set_time_of_day` `WrongState`).

use super::ServerState;
use crate::sync::SYNC;
use bedrock_sync::clock::ClockServer;
use wire_weaver::prelude::*;
use ww_error::CommonError;
use ww_si::ww_numeric::NumericValue;
use ww_si::{Prefix, Second};
use ww_timebase::{
    ClockError, ClockInfo, DateTime, SteerPoint, SyncConfig, SyncSource, SyncStatus,
};

/// Local or shared time as `clock` writes it: `U32` microseconds, wrapping every 71.6 minutes (Cortex-M0+, no
/// 64-bit `ww_numeric`). `now()` and `info()` use it too, so `time_map` and `sync_status` come in the same prefix
/// and storage.
fn to_second(us: u64) -> Second {
    Second::from_parts(Prefix::Micro, NumericValue::U32(us as u32))
}

/// `clock`'s state besides `crate::sync::SYNC`: the sync line's configuration (always `Off`) and whether a host is
/// subscribed to `time_map`.
pub(super) struct ClockState {
    server: ClockServer,
    time_map_open: bool,
}

impl ClockState {
    pub(super) const fn new() -> Self {
        ClockState {
            server: ClockServer::new(to_second),
            time_map_open: false,
        }
    }
}

// ww_timebase::Clock at clock
impl ServerState {
    pub(super) fn clock_now(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<Second> {
        Ready(to_second(bedrock_sync::Sync::local_us()))
    }

    pub(super) fn clock_info(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<ClockInfo<'_>> {
        Ready(ClockInfo {
            resolution: to_second(1),
            source: MaxLen::new_unchecked("TIM4"),
            sync_formats: MaxLen::new_unchecked(RefVec::Slice {
                slice: ClockServer::NO_LINE,
            }),
        })
    }

    pub(super) fn clock_time_map(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        self.clock.time_map_open = true;
        Ready(())
    }

    pub(super) fn close_clock_time_map(&mut self, _cx: &mut Context<'_, impl EventOut>) {
        self.clock.time_map_open = false;
    }

    pub(super) fn sideband_clock_time_map(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        None
    }

    pub(super) fn clock_configure_sync(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        config: SyncConfig,
    ) -> RpcResult<Result<(), ClockError<'_>>> {
        Ready(
            self.clock
                .server
                .configure_sync(config, ClockServer::NO_LINE),
        )
    }

    pub(super) fn clock_sync_config(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<SyncConfig> {
        Ready(self.clock.server.sync_config())
    }

    pub(super) fn clock_sync_status(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<SyncStatus<'_>> {
        Ready(SYNC.with(|clock, now_us| self.clock.server.sync_status(clock, now_us)))
    }

    /// Never a sync master (no line): always `WrongState`.
    pub(super) fn clock_set_time_of_day(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        _t: DateTime,
    ) -> RpcResult<Result<(), ClockError<'_>>> {
        Ready(Err(CommonError::WrongState {
            reason: MaxLen::new_unchecked("no sync line, never a master"),
        }
        .into()))
    }

    /// A `time_map` point is sent right after, if a host is subscribed: the host steers about once a second while
    /// connected, which is the "about once a second while synced" `time_map` asks for.
    pub(super) async fn clock_steer(
        &mut self,
        cx: &mut Context<'_, impl EventOut>,
        point: SteerPoint<'_>,
    ) -> RpcResult<Result<Option<SyncSource>, ClockError<'_>>> {
        let result = SYNC.with(|clock, now_us| self.clock.server.steer(clock, now_us, &point));
        if result.is_ok()
            && self.clock.time_map_open
            && let Some(tp) =
                SYNC.with(|clock, now_us| self.clock.server.time_map_point(clock, now_us))
        {
            let _ = super::server_impl::stream_data_ser()
                .clock()
                .time_map_send(&tp, cx)
                .await;
        }
        Ready(result)
    }

    pub(super) fn clock_shared_now(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<Option<u64>> {
        Ready(SYNC.with(|clock, now_us| self.clock.server.shared_now(clock, now_us)))
    }

    /// No sync line to list: the only lists that mean something are the default (`None`) and `[Host]`.
    pub(super) fn clock_set_sources(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
        sources: Option<MaxLen<RefVec<'_, SyncSource>, 8>>,
    ) -> RpcResult<Result<(), ClockError<'_>>> {
        Ready(SYNC.with(|clock, _now_us| {
            self.clock
                .server
                .set_sources(clock, sources, ClockServer::NO_LINE)
        }))
    }

    pub(super) fn clock_sources(
        &mut self,
        _cx: &mut Context<'_, impl EventOut>,
    ) -> RpcResult<Option<MaxLen<RefVec<'_, SyncSource>, 8>>> {
        Ready(self.clock.server.sources())
    }
}
