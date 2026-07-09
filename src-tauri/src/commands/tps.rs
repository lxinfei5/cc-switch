//! TPS 监控相关命令（本地定制）
//!
//! 薄封装层：调用 `state.db`（数据访问见 `services/tps.rs`）与 `crate::tps`
//! （开关/事件编排）。命令名注册到 `lib.rs` 的 `generate_handler!`。

use crate::error::AppError;
use crate::services::tps::{TpsFilters, TpsSample, TpsSummary, TpsTrendPoint};
use crate::store::AppState;
use crate::tps;
use tauri::State;

/// 获取 TPS 汇总（count / avg / max / p50 / p95 等）
#[tauri::command]
pub fn get_tps_summary(
    state: State<'_, AppState>,
    filters: TpsFilters,
) -> Result<TpsSummary, AppError> {
    state.db.get_tps_summary(&filters)
}

/// 获取最近 N 条 TPS 样本（默认 100）
#[tauri::command]
pub fn get_tps_recent(
    state: State<'_, AppState>,
    filters: TpsFilters,
    limit: Option<u32>,
) -> Result<Vec<TpsSample>, AppError> {
    state.db.get_tps_recent(&filters, limit.unwrap_or(100))
}

/// 获取分桶 TPS 趋势（默认 48 桶）
#[tauri::command]
pub fn get_tps_trend(
    state: State<'_, AppState>,
    filters: TpsFilters,
    buckets: Option<u32>,
) -> Result<Vec<TpsTrendPoint>, AppError> {
    state.db.get_tps_trend(&filters, buckets.unwrap_or(48))
}

/// 清空 TPS 样本（filters 为空则清空全部），返回删除行数
#[tauri::command]
pub fn clear_tps_samples(
    state: State<'_, AppState>,
    filters: Option<TpsFilters>,
) -> Result<u64, AppError> {
    state.db.clear_tps_samples(&filters.unwrap_or_default())
}

/// TPS 样本总条数
#[tauri::command]
pub fn count_tps_samples(state: State<'_, AppState>) -> Result<u64, AppError> {
    state.db.count_tps_samples()
}

/// 查询 TPS 记录是否开启
#[tauri::command]
pub fn get_tps_enabled() -> bool {
    tps::is_enabled()
}

/// 开启/关闭 TPS 记录（持久化到 settings 表）
#[tauri::command]
pub fn set_tps_enabled(state: State<'_, AppState>, enabled: bool) -> Result<(), AppError> {
    tps::set_enabled(&state.db, enabled)
}