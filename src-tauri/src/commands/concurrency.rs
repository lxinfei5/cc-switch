//! 并发监控命令（本地定制，fork 专用）
//!
//! 薄封装：并发跟踪器是进程级单例（见 [`crate::concurrency`]），命令无需 AppState。

use crate::concurrency::{self, ConcurrencySnapshot};

/// 获取当前并发快照（current / peak / 最近历史采样）
#[tauri::command]
pub fn get_concurrency() -> ConcurrencySnapshot {
    concurrency::tracker()
        .map(|t| t.snapshot())
        .unwrap_or_default()
}

/// 重置峰值并发基准（之后重新统计新高）
#[tauri::command]
pub fn reset_concurrency_peak() {
    if let Some(t) = concurrency::tracker() {
        t.reset_peak();
    }
}
