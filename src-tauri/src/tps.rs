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

/// 从 RequestLog 派生 tokens/sec（**单次请求**，不跨并发聚合）。
///
/// 公式：`output_tokens / generation_seconds`，其中 generation 优先取
/// `latency_ms - first_token_ms`（首 token 之后的纯生成窗口）；不可信时退回
/// 整段 `latency_ms`。
///
/// 重要语义：
/// - 每条样本对应一次代理请求（一次对话 turn），并发请求各自独立采样，
///   **绝不会**把多路并发的 token 加总到同一个 TPS 分母/分子上。
/// - 历史上 `first_token_ms` 曾以「首个 usage 相关 SSE 事件」计时：OpenAI /
///   Codex / Gemini 的 usage 事件通常在流末尾，生成窗口塌缩为几毫秒，
///   会出现 8000+ tok/s 的离群值。现已在流式路径上改为「上游首字节」计时，
///   并在此对残留的「伪生成窗口」做启发式兜底。
fn compute_tps(log: &RequestLog) -> f64 {
    let output = log.usage.output_tokens;
    if output == 0 {
        return 0.0;
    }
    let gen_ms = generation_ms(log.is_streaming, log.first_token_ms, log.latency_ms);
    if gen_ms == 0 {
        return 0.0;
    }
    output as f64 * 1000.0 / gen_ms as f64
}

/// 计算用于 TPS 分母的生成窗口（毫秒）。
///
/// - 流式且 first_token 可信：`latency - first_token`
/// - 否则：整段 `latency`（至少 1ms）
///
/// 「不可信」判定：first_token 缺失 / 越过总耗时 / 生成窗口相对总耗时过窄
/// （典型症状：usage 事件被当成 first_token，落在流结束前几毫秒）。
fn generation_ms(is_streaming: bool, first_token_ms: Option<u64>, latency_ms: u64) -> u64 {
    let latency = latency_ms.max(1);
    if !is_streaming {
        return latency;
    }
    match first_token_ms {
        None => latency,
        // first_token >= 总耗时：测量无效，回退整段 latency（不再返回 0：
        // 否则短响应会被丢弃，用户看到 tps=0 却无法用输出/耗时反推）。
        Some(ftt) if ftt >= latency => latency,
        Some(ftt) => {
            let gen = latency - ftt;
            if is_unreliable_generation_window(gen, latency) {
                latency
            } else {
                gen
            }
        }
    }
}

/// 判断 (latency - first_token) 是否像「伪生成窗口」而非真实生成耗时。
///
/// 真实模型生成几十/上百 token 极少在 <20ms 内完成；若同时总耗时较长，
/// 几乎可以肯定 first_token 落在了流末 usage 事件上。
fn is_unreliable_generation_window(gen_ms: u64, latency_ms: u64) -> bool {
    // 绝对阈值：生成窗口极短且总耗时非瞬时
    if gen_ms < 20 && latency_ms > 100 {
        return true;
    }
    // 相对阈值：生成窗口 < 总耗时的 2%（且总耗时至少 500ms）
    if latency_ms >= 500 && gen_ms.saturating_mul(50) < latency_ms {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::usage::parser::TokenUsage;

    fn sample_log(
        output: u32,
        latency_ms: u64,
        first_token_ms: Option<u64>,
        is_streaming: bool,
    ) -> RequestLog {
        RequestLog {
            request_id: "test".into(),
            provider_id: "p".into(),
            app_type: "claude".into(),
            model: "m".into(),
            request_model: "m".into(),
            pricing_model: "m".into(),
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: output,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                model: None,
                message_id: None,
            },
            cost: None,
            latency_ms,
            first_token_ms,
            status_code: 200,
            error_message: None,
            session_id: None,
            provider_type: None,
            is_streaming,
            cost_multiplier: "1.0".into(),
        }
    }

    #[test]
    fn tps_is_per_request_using_full_latency_when_no_first_token() {
        // 800 tokens / 2s = 400 tok/s
        let log = sample_log(800, 2000, None, true);
        let tps = compute_tps(&log);
        assert!((tps - 400.0).abs() < 0.01, "tps={tps}");
    }

    #[test]
    fn tps_subtracts_reliable_first_token() {
        // 900 tokens, total 3s, first token at 1s → gen=2s → 450 tok/s
        let log = sample_log(900, 3000, Some(1000), true);
        let tps = compute_tps(&log);
        assert!((tps - 450.0).abs() < 0.01, "tps={tps}");
    }

    #[test]
    fn tps_falls_back_when_first_token_is_late_usage_event() {
        // 典型 OpenAI/Codex 旧语义：first_token 落在流末，gen=5ms。
        // 若按 gen 计算会得到 800*1000/5 = 160000 tok/s（荒谬）。
        // 启发式应回退到整段 latency：800 / 2s = 400.
        let log = sample_log(800, 2000, Some(1995), true);
        let tps = compute_tps(&log);
        assert!(
            (tps - 400.0).abs() < 0.01,
            "expected fallback 400 tok/s, got {tps}"
        );
        // 确认未走「返回 0」的旧路径
        assert!(tps > 0.0);
    }

    #[test]
    fn tps_falls_back_when_first_token_ge_latency() {
        let log = sample_log(100, 1000, Some(1000), true);
        let tps = compute_tps(&log);
        // 100 tokens / 1s = 100
        assert!((tps - 100.0).abs() < 0.01, "tps={tps}");
    }

    #[test]
    fn tps_non_streaming_uses_full_latency() {
        let log = sample_log(500, 2500, Some(10), false);
        let tps = compute_tps(&log);
        // first_token 对非流式忽略：500 / 2.5s = 200
        assert!((tps - 200.0).abs() < 0.01, "tps={tps}");
    }

    #[test]
    fn tps_zero_output_is_zero() {
        let log = sample_log(0, 1000, Some(100), true);
        assert_eq!(compute_tps(&log), 0.0);
    }

    #[test]
    fn generation_ms_accepts_short_but_plausible_window() {
        // 80ms 生成 + 200ms 总耗时：短响应但合理，应保留 gen 窗口
        assert_eq!(generation_ms(true, Some(120), 200), 80);
    }

    #[test]
    fn concurrent_requests_are_independent_samples() {
        // 两路并发各自 400 tok/s；聚合侧取平均仍是 400，绝不会变成 800。
        // （本函数只负责单样本；这里验证单样本不会「吃掉」另一路的时间。）
        let a = sample_log(400, 1000, None, true);
        let b = sample_log(800, 2000, None, true);
        let tps_a = compute_tps(&a);
        let tps_b = compute_tps(&b);
        assert!((tps_a - 400.0).abs() < 0.01);
        assert!((tps_b - 400.0).abs() < 0.01);
        // 若错误地用 wall-clock 共享窗口会得到 (400+800)/max(1,2)s 等错误值
        assert!(((tps_a + tps_b) / 2.0 - 400.0).abs() < 0.01);
    }
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
