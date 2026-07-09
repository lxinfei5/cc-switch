//! 并发数监控 —— 本地定制（fork 专用）
//!
//! 跟踪本地代理当前正在处理的请求数（in-flight）及其峰值，以及最近一段时间的
//! 并发采样序列。与 TPS 监控解耦：TPS 是「每秒 token 数」（请求结束后落库），
//! 并发是「同时在途请求数」（实时计数），二者互不依赖。
//!
//! 接入点：仅在 `proxy/server.rs::build_router` 末尾追加一行
//! `.layer(axum::middleware::from_fn(concurrency_middleware))`。中间件在请求进入时
//! `enter()`，并把响应体包成 [`ConcurrencyBody`]，当响应体被消费/丢弃时 `leave()`
//! （Drop 触发），从而对流式与非流式、正常与异常路径都能成对增减，不泄漏计数。
//!
//! 移除：删掉本文件 + `commands/concurrency.rs` + server.rs 的 `.layer(...)` 一行
//! + lib.rs 的 `mod concurrency; concurrency::init();` 与命令注册。

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use bytes::Bytes;
use http_body::Frame;
use serde::Serialize;

/// 历史采样保留条数（1 秒一条 → 约 4 分钟滚动窗口）
const HISTORY_CAP: usize = 240;

/// 并发计数器内部状态（共享、Clone 廉价 —— Arc）
struct Inner {
    in_flight: AtomicI64,
    peak: AtomicI64,
    history: Mutex<VecDeque<(i64, i64)>>,
}

/// 并发跟踪器（Clone = 复制 Arc，廉价）
#[derive(Clone)]
pub struct ConcurrencyTracker {
    inner: std::sync::Arc<Inner>,
}

impl ConcurrencyTracker {
    fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(Inner {
                in_flight: AtomicI64::new(0),
                peak: AtomicI64::new(0),
                history: Mutex::new(VecDeque::new()),
            }),
        }
    }

    /// 请求进入：in-flight +1 并刷新峰值，返回一个 RAII 守卫。
    ///
    /// 守卫 Drop 时调用 `leave()`（且仅一次）。把守卫移入 [`ConcurrencyBody`]
    /// 可让计数与响应体生命周期对齐（流式响应在最后一帧/客户端断开才结束）；
    /// 若在构造 ConcurrencyBody 之前发生 panic，守卫在 unwind 时 Drop 也会
    /// 触发 `leave()`，保证 enter/leave 严格成对、不泄漏计数。
    pub fn enter(&self) -> InFlightGuard {
        let n = self.inner.in_flight.fetch_add(1, Ordering::AcqRel) + 1;
        // fetch_max：保持历史峰值（线程安全的 CAS 循环，由标准库实现）
        let _ = self.inner.peak.fetch_max(n, Ordering::AcqRel);
        InFlightGuard::new(self.clone())
    }

    /// 请求结束：in-flight -1（下限 0）。
    pub fn leave(&self) {
        // fetch_sub 后若越界（不应发生，enter/leave 成对），钳到 0。
        let prev = self.inner.in_flight.fetch_sub(1, Ordering::AcqRel);
        if prev <= 0 {
            // 修正：把负数/零回填为 0，避免计数器漂移
            self.inner.in_flight.store(0, Ordering::Release);
        }
    }

    /// 当前在途并发数。
    pub fn current(&self) -> i64 {
        self.inner.in_flight.load(Ordering::Acquire).max(0)
    }

    /// 自启动/上次重置以来的峰值并发。
    pub fn peak(&self) -> i64 {
        self.inner.peak.load(Ordering::Acquire).max(0)
    }

    /// 重置峰值基准为当前在途数（之后重新统计新高）。
    pub fn reset_peak(&self) {
        self.inner.peak.store(self.current(), Ordering::Release);
    }

    /// 追加一条历史采样（由后台采样器调用）。
    fn record_history(&self, ts: i64, count: i64) {
        if let Ok(mut hist) = self.inner.history.lock() {
            hist.push_back((ts, count));
            while hist.len() > HISTORY_CAP {
                hist.pop_front();
            }
        }
    }

    /// 历史采样快照（拷贝出来供前端绘制迷你趋势）。
    pub fn history_snapshot(&self) -> Vec<ConcurrencySample> {
        self.inner
            .history
            .lock()
            .map(|h| {
                h.iter()
                    .map(|&(ts, c)| ConcurrencySample { ts, count: c })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 完整快照。
    pub fn snapshot(&self) -> ConcurrencySnapshot {
        ConcurrencySnapshot {
            current: self.current(),
            peak: self.peak(),
            history: self.history_snapshot(),
        }
    }
}

/// 全局跟踪器单例（setup 阶段 init 一次）。
static TRACKER: OnceLock<ConcurrencyTracker> = OnceLock::new();

/// 取全局跟踪器（init 之前或未启用时返回 None，中间件将不计数）。
pub fn tracker() -> Option<ConcurrencyTracker> {
    TRACKER.get().cloned()
}

/// 在应用 setup 阶段调用一次：注入跟踪器并启动 1 秒后台采样器。
///
/// 重复调用无害（OnceLock 仅首次写入生效）。采样器在 tauri 异步运行时上常驻，
/// 每 1 秒记录一次 (timestamp, in_flight) 到滚动历史。
pub fn init() {
    if TRACKER.set(ConcurrencyTracker::new()).is_err() {
        log::debug!("[concurrency] init 重复调用，已忽略");
        return;
    }
    log::info!("[concurrency] 跟踪器已注入，并发采样器启动");

    let Some(tracker) = TRACKER.get().cloned() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.tick().await; // 跳过立即首次触发
        loop {
            interval.tick().await;
            let now = chrono::Utc::now().timestamp();
            tracker.record_history(now, tracker.current());
        }
    });
}

// ── 前端返回结构 ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConcurrencySample {
    /// unix 秒
    pub ts: i64,
    /// 当时在途并发数
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConcurrencySnapshot {
    pub current: i64,
    pub peak: i64,
    pub history: Vec<ConcurrencySample>,
}

// ── 中间件 ─────────────────────────────────────────────────────────────────

/// RAII 守卫：Drop 时调用一次 `leave()`。`active` 防止重复 leave。
pub struct InFlightGuard {
    tracker: ConcurrencyTracker,
    active: bool,
}

impl InFlightGuard {
    fn new(tracker: ConcurrencyTracker) -> Self {
        Self {
            tracker,
            active: true,
        }
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if self.active {
            self.active = false;
            self.tracker.leave();
        }
    }
}

/// axum 中间件：请求进入 enter()（返回 RAII 守卫），响应体包成 ConcurrencyBody，
/// 守卫移入体——体被消费/丢弃时 leave()（Drop）。若 next.run 在此 panic，
/// 守卫在 unwind 时 Drop 也会 leave()，保证成对计数。
pub async fn concurrency_middleware(req: Request, next: Next) -> Response {
    let Some(tracker) = tracker() else {
        // 未启用：直接透传，不计数。
        return next.run(req).await;
    };
    let guard = tracker.enter();
    let res = next.run(req).await;
    let (parts, body) = res.into_parts();
    Response::from_parts(parts, Body::new(ConcurrencyBody { inner: body, guard }))
}

/// 响应体包装：转发帧，Drop 时由 `guard` 字段自动 leave()。
/// 保证流式响应在最后一帧/客户端断开后才计数结束。
pub(crate) struct ConcurrencyBody {
    inner: Body,
    // 仅为其 Drop 副作用（leave()）而存在，故允许 dead_code。
    #[allow(dead_code)]
    guard: InFlightGuard,
}

impl http_body::Body for ConcurrencyBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        // 安全：仅把 Pin 投影到 inner 字段；ConcurrencyBody 没有其他 pin 相关状态，
        // Drop 只让 guard 字段 leave()，不会移动 inner。
        let inner = unsafe { self.map_unchecked_mut(|this| &mut this.inner) };
        inner.poll_frame(cx)
    }
    // 无需手动 Drop：ConcurrencyBody 被丢弃时 guard 字段自动 Drop → leave()。
}
