//! TPS 监控 & 记录 —— 编排层（本地定制，fork 专用）
//!
//! 本模块是 TPS 功能对核心代理路径的**唯一入口**：核心仅在
//! `proxy/usage/logger.rs::log_request` 落库后调用一次 [`on_request_logged`]，
//! 其余逻辑（开关、计算、写库、事件推送）全部封装在此与 `services/tps.rs`。
//!
//! 解耦设计：要移除 TPS 功能，只需删除本文件 + `services/tps.rs` +
//! `commands/tps.rs` + `lib.rs`/`services/mod.rs`/`commands/mod.rs` 里的几行声明、
//! `logger.rs` 里的一行 hook 调用、`schema.rs` 里的建表/迁移，互不影响成本核算。
//!
//! - 开关：进程内 `AtomicBool`（默认开启），可由前端 `set_tps_enabled` 命令切换并
//!   持久化到 settings 表；启动时由 [`load_enabled`] 从 settings 读回。
//! - 计算：`output_tokens / generation_seconds`，流式时 generation = latency - first_token。
//! - 事件：`tps-recorded`，200ms 防抖合并，镜像 `usage_events` 的实现。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::database::Database;
use crate::error::AppError;
use crate::proxy::usage::logger::RequestLog;
use crate::services::tps::TpsSampleInput;

/// 前端监听的事件名
pub const EVENT_TPS_RECORDED: &str = "tps-recorded";

/// settings 表中持久化开关的 key
const SETTING_TPS_ENABLED: &str = "tps_monitor_enabled";

/// 防抖窗口：合并 200ms 内的多次通知。
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(200);

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// TPS 记录开关（进程内快路径，避免每请求查 DB）。默认开启。
static ENABLED: AtomicBool = AtomicBool::new(true);

/// 事件防抖标记：true 表示已有调度任务在等待 emit。
static EMIT_SCHEDULED: AtomicBool = AtomicBool::new(false);

/// 在应用 setup 阶段调用一次，注入 AppHandle（用于事件推送）。
///
/// 重复调用无害（OnceLock 仅首次写入生效）。不读取 DB，因此可在数据库初始化之前调用。
pub fn init(handle: AppHandle) {
    if APP_HANDLE.set(handle).is_err() {
        log::debug!("[tps] init 重复调用，已忽略");
    } else {
        log::info!("[tps] AppHandle 已注入，TPS 事件推送启用");
    }
}

/// 数据库初始化后调用一次，从 settings 读回开关持久化值。
pub fn load_enabled(db: &Database) {
    let enabled = match db.get_setting(SETTING_TPS_ENABLED) {
        Ok(Some(v)) => v == "true" || v == "1",
        Ok(None) => true, // 未设置过，默认开启
        Err(e) => {
            log::warn!("[tps] 读取开关设置失败，默认开启: {e}");
            true
        }
    };
    ENABLED.store(enabled, Ordering::Release);
    log::info!("[tps] 监控开关已加载: enabled={enabled}");
}

/// 当前是否启用 TPS 记录（热路径读取，O(1)）。
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// 设置开关并持久化到 settings 表。
pub fn set_enabled(db: &Database, enabled: bool) -> Result<(), AppError> {
    ENABLED.store(enabled, Ordering::Release);
    db.set_setting(SETTING_TPS_ENABLED, if enabled { "true" } else { "false" })?;
    log::info!("[tps] 监控开关已更新: enabled={enabled}");
    Ok(())
}

/// 从 RequestLog 派生 tokens/sec。
///
/// - 流式且有 first_token_ms：generation = latency_ms - first_token_ms（纯生成耗时）
/// - 其它：generation = latency_ms
/// - output_tokens 为 0 或 generation <= 0：返回 0.0
fn compute_tps(log: &RequestLog) -> f64 {
    let output = log.usage.output_tokens;
    if output == 0 {
        return 0.0;
    }
    let gen_ms: u64 = if log.is_streaming {
        match log.first_token_ms {
            // 首 token 时间缺失：用整段 latency 兜底（保守下界）。
            None => log.latency_ms.max(1),
            // 首 token 时间 >= 总耗时：生成耗时不可测（毫秒截断下首 token 紧贴
            // 完成时可达）。视为无效测量返回 0.0，避免 .max(1) 把 0ms 生成
            // 放大成 output * 1000 的离群值污染峰值/P95/趋势。
            Some(ftt) if ftt >= log.latency_ms => return 0.0,
            // 正常：生成耗时 = 总耗时 - 首 token 耗时（ftt < latency，保证 >= 1ms）。
            Some(ftt) => log.latency_ms - ftt,
        }
    } else {
        log.latency_ms.max(1)
    };
    output as f64 * 1000.0 / gen_ms as f64
}

/// 核心代理路径的唯一钩子：在 `proxy_request_logs` 写入后由 logger 调用。
///
/// 失败仅记录 warn，绝不向上传播——TPS 是可观测性附加，不能影响计费主路径。
/// 开关关闭时直接跳过，零开销。
pub fn on_request_logged(db: &Database, log: &RequestLog) -> Result<(), AppError> {
    if !is_enabled() {
        return Ok(());
    }

    // 错误请求 / 未生成 output 的请求：无 TPS 可言，跳过采样，
    // 避免把 tps=0 的行写入 tps_samples 而虚增 sample_count / 拖低趋势均值。
    if log.usage.output_tokens == 0 {
        return Ok(());
    }

    let input = TpsSampleInput {
        request_id: log.request_id.clone(),
        app_type: log.app_type.clone(),
        provider_id: log.provider_id.clone(),
        model: log.model.clone(),
        output_tokens: log.usage.output_tokens as i64,
        first_token_ms: log.first_token_ms.map(|v| v as i64),
        duration_ms: Some(log.latency_ms as i64),
        is_streaming: log.is_streaming,
        tps: compute_tps(log),
        created_at: chrono::Utc::now().timestamp(),
    };

    db.insert_tps_sample(&input)?;
    notify_tps_recorded();
    Ok(())
}

/// 通知前端有新的 TPS 样本（200ms 防抖合并）。
fn notify_tps_recorded() {
    let Some(handle) = APP_HANDLE.get() else {
        return;
    };
    if EMIT_SCHEDULED.swap(true, Ordering::AcqRel) {
        return;
    }
    let handle = handle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(DEBOUNCE_WINDOW);
        EMIT_SCHEDULED.store(false, Ordering::Release);
        if let Err(e) = handle.emit(EVENT_TPS_RECORDED, ()) {
            log::warn!("[tps] emit {EVENT_TPS_RECORDED} 失败: {e}");
        }
    });
}
