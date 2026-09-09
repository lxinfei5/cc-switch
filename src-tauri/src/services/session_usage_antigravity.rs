//! Antigravity 会话用量追踪（usage-only）
//!
//! 从 `~/.gemini/{antigravity,antigravity-cli,antigravity-ide}/conversations/*.db`
//! 的 `gen_metadata` protobuf 提取 token，写入 `proxy_request_logs`。
//! 不把 Antigravity 做成受管应用，也不改 Gemini CLI JSON 解析器。
//!
//! ## 数据流
//! ```text
//! conversations/*.db（只读 + WAL mtime，不含 SHM）
//!   → gen_metadata protobuf（f2 fresh input / f3 output / f5 cache read）
//!   → 费用计算 → proxy_request_logs（app_type=antigravity）
//! ```
//!
//! ## 字段口径
//! Agy `gen_metadata`：f2 = 未缓存输入，f3 = 含 thinking 的完整输出，f5 = cache read。
//! 本导入器按 fresh-input 落库（`input_token_semantics = FRESH`），不把 cache
//! 加进 `input_tokens`。这与 Gemini 代理行的 cache-inclusive 口径不同，因此
//! 使用独立 `app_type=antigravity`，避免污染 Gemini 汇总。
//!
//! ## 增量
//! - 检查点在 `session_log_sync`：`last_modified` 取 db+WAL mtime（**不含 SHM**：
//!   只读打开会自己写 SHM，把它算进水位会每分钟重扫全部历史库）。
//! - `last_line_offset` 是下一个待处理的 `gen_metadata.idx`。
//! - 非终态 step（本机生成中为 status 2 或 8；终态为 3/5/6/7）不插入对应
//!   gen、不把检查点推过该 idx。已完成的前缀仍可推进，避免活跃会话拖住整库。
//!
//! ## 上游合入时必须 wipe（break-glass）
//! 身份是 `request_id=antigravity_session:{sid}:{idx}` / `app_type=antigravity`。
//! 上游 PR #5230 用 `gemini_antigravity_session:` + `app_type=gemini`。两个
//! importer 并存会双计。合入 5230/5975 时 **禁止只删本文件**：必须同时
//! `DELETE` `proxy_request_logs`（`data_source='antigravity_session'`）、
//! `usage_daily_rollups`（`app_type='antigravity'`）、以及这些 conversation
//! `.db` 的 `session_log_sync` 游标，再让上游 importer 重导。没有 Antigravity
//! 专用 rebuild 按钮。
//!
//! 解析逻辑移植自上游 PR #5230；独立成模块是为了最小合入。展示层过滤来自 PR #4202。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::gemini_config::get_gemini_dir;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    load_sync_cursors, metadata_modified_nanos, update_sync_state_on_conn, SessionSyncResult,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH;
use crate::services::usage_stats::{find_model_pricing, is_placeholder_pricing_model};
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const APP_TYPE: &str = "antigravity";
const DATA_SOURCE: &str = "antigravity_session";
const PROVIDER_ID: &str = "_antigravity_session";

const ANTIGRAVITY_ROOTS: [&str; 3] = ["antigravity", "antigravity-cli", "antigravity-ide"];
/// Live generating step on this CLI (step_type=15). status=2 also appears briefly.
const ANTIGRAVITY_STATUS_GENERATING: i64 = 8;
const MAX_GEN_BLOB_BYTES: usize = 2 * 1024 * 1024;
const BUSY_TIMEOUT: Duration = Duration::from_millis(200);

fn is_terminal_status(status: i64) -> bool {
    matches!(status, 3 | 5 | 6 | 7)
}

fn is_sqlite_busy(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(e, _)
            if matches!(
                e.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
    )
}

#[derive(Debug, Clone)]
enum ProtoValue<'a> {
    Varint(u64),
    LengthDelimited(&'a [u8]),
}

struct ProtoParser<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> ProtoParser<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn decode_varint(&mut self) -> Option<u64> {
        let mut result = 0u64;
        let mut shift = 0u32;
        while self.offset < self.data.len() {
            let byte = self.data[self.offset];
            self.offset += 1;
            result |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Some(result);
            }
            shift += 7;
            if shift >= 64 {
                return None;
            }
        }
        None
    }

    fn next_field(&mut self) -> Option<(u32, ProtoValue<'a>)> {
        while self.offset < self.data.len() {
            let tag = self.decode_varint()?;
            let field_num = (tag >> 3) as u32;
            let wire_type = (tag & 0x7) as u32;

            match wire_type {
                0 => {
                    return self
                        .decode_varint()
                        .map(|value| (field_num, ProtoValue::Varint(value)));
                }
                1 => {
                    if self.offset + 8 > self.data.len() {
                        return None;
                    }
                    self.offset += 8;
                }
                2 => {
                    let length = self.decode_varint()? as usize;
                    let end = match self.offset.checked_add(length) {
                        Some(end) if end <= self.data.len() => end,
                        _ => return None,
                    };
                    let blob = &self.data[self.offset..end];
                    self.offset = end;
                    return Some((field_num, ProtoValue::LengthDelimited(blob)));
                }
                3 | 4 => {}
                5 => {
                    if self.offset + 4 > self.data.len() {
                        return None;
                    }
                    self.offset += 4;
                }
                _ => {
                    // Unknown wire: stop scanning this message but keep fields
                    // already yielded. Aborting the whole parse dropped rows
                    // when an unknown tag appeared before field 1.
                    return None;
                }
            }
        }
        None
    }

    fn get_varint(&mut self, target_field: u32) -> Option<u64> {
        while let Some((field, value)) = self.next_field() {
            if field == target_field {
                if let ProtoValue::Varint(value) = value {
                    return Some(value);
                }
            }
        }
        None
    }

    fn get_nested(&mut self, target_field: u32) -> Option<&'a [u8]> {
        while let Some((field, value)) = self.next_field() {
            if field == target_field {
                if let ProtoValue::LengthDelimited(val) = value {
                    return Some(val);
                }
            }
        }
        None
    }
}

#[derive(Debug, Default)]
struct AntigravityTokenData {
    /// Raw `gen_metadata` field f2: fresh input only.
    input_tokens: u32,
    output_tokens: u32,
    /// Raw `gen_metadata` field f5: cache-read input.
    cached_tokens: u32,
    model: String,
}

impl AntigravityTokenData {
    fn has_tokens(&self) -> bool {
        self.input_tokens != 0 || self.output_tokens != 0 || self.cached_tokens != 0
    }
}

#[derive(Debug, Default)]
struct TrajectoryMetadata {
    session_id: Option<String>,
    created_at_seconds: Option<i64>,
}

#[derive(Debug)]
struct GenMetadataEntry {
    idx: i64,
    token_data: Option<AntigravityTokenData>,
    parse_failed: bool,
}

#[derive(Debug, Default, Clone, Copy)]
struct AntigravityStepState {
    timestamp: Option<i64>,
    running: bool,
}

struct AntigravityStepStates {
    by_gen_idx: HashMap<i64, AntigravityStepState>,
    running_gen_idxs: HashSet<i64>,
    has_running_step: bool,
    has_unmapped_running: bool,
}

struct SingleFileSync {
    imported: u32,
    skipped: u32,
    deferred: bool,
}

pub fn sync_antigravity_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let gemini_dir = get_gemini_dir();
    let files = collect_antigravity_db_files(&gemini_dir);
    let mut result = SessionSyncResult {
        files_scanned: files.len() as u32,
        ..Default::default()
    };
    if files.is_empty() {
        return Ok(result);
    }

    let cursors = load_sync_cursors(db)?;
    for file_path in &files {
        let cursor = cursors
            .get(file_path.to_string_lossy().as_ref())
            .copied()
            .unwrap_or_default();
        match sync_single_antigravity_db(
            db,
            file_path,
            cursor.last_modified,
            cursor.last_line_offset,
        ) {
            Ok(step) => {
                result.imported += step.imported;
                result.skipped += step.skipped;
                if step.deferred {
                    result.deferred_files += 1;
                }
            }
            Err(e) => {
                let msg = format!(
                    "Antigravity 会话数据库解析失败 {}: {e}",
                    file_path.display()
                );
                log::warn!("[ANTIGRAVITY-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[ANTIGRAVITY-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件, 推迟 {} 个",
            result.imported,
            result.skipped,
            result.files_scanned,
            result.deferred_files
        );
    }

    Ok(result)
}

fn collect_antigravity_db_files(gemini_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in ANTIGRAVITY_ROOTS {
        let conversations_dir = gemini_dir.join(root).join("conversations");
        let entries = match fs::read_dir(&conversations_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if fs::symlink_metadata(&path)
                .map(|meta| meta.file_type().is_symlink())
                .unwrap_or(false)
            {
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) == Some("db") {
                files.push(path);
            }
        }
    }
    files
}

fn composite_modified_nanos(db_path: &Path) -> Result<i64, AppError> {
    let metadata =
        fs::metadata(db_path).map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let mut file_modified = metadata_modified_nanos(&metadata);
    let wal_path = PathBuf::from(format!("{}-wal", db_path.to_string_lossy()));
    if let Ok(wal_meta) = fs::metadata(&wal_path) {
        file_modified = file_modified.max(metadata_modified_nanos(&wal_meta));
    }
    Ok(file_modified)
}

fn sync_single_antigravity_db(
    db: &Database,
    db_path: &Path,
    last_modified: i64,
    last_gen_idx: i64,
) -> Result<SingleFileSync, AppError> {
    let file_path_str = db_path.to_string_lossy().to_string();
    let file_modified = composite_modified_nanos(db_path)?;
    if file_modified <= last_modified {
        return Ok(SingleFileSync {
            imported: 0,
            skipped: 0,
            deferred: false,
        });
    }
    let file_modified_secs = (file_modified / 1_000_000_000) as i64;

    let mut agy_conn = match rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(conn) => conn,
        Err(e) if is_sqlite_busy(&e) => {
            return Ok(SingleFileSync {
                imported: 0,
                skipped: 0,
                deferred: true,
            });
        }
        Err(e) => {
            return Err(AppError::Config(format!(
                "无法只读打开 Antigravity DB: {e}"
            )));
        }
    };

    let _ = agy_conn.busy_timeout(BUSY_TIMEOUT);

    let tx = match agy_conn.transaction_with_behavior(rusqlite::TransactionBehavior::Deferred) {
        Ok(tx) => tx,
        Err(e) if is_sqlite_busy(&e) => {
            return Ok(SingleFileSync {
                imported: 0,
                skipped: 0,
                deferred: true,
            });
        }
        Err(e) => return Err(AppError::Database(format!("无法开启只读事务: {e}"))),
    };

    let trajectory_meta = read_trajectory_metadata(&tx);
    let step_states = match read_step_states(&tx) {
        Ok(states) => states,
        Err(e) if is_sqlite_busy(&e) => {
            return Ok(SingleFileSync {
                imported: 0,
                skipped: 0,
                deferred: true,
            });
        }
        Err(e) => {
            return Err(AppError::Database(format!(
                "读取 Antigravity steps 失败: {e}"
            )));
        }
    };
    let gen_entries = match read_gen_metadata_entries(&tx, last_gen_idx) {
        Ok(entries) => entries,
        Err(e) if is_sqlite_busy(&e) => {
            return Ok(SingleFileSync {
                imported: 0,
                skipped: 0,
                deferred: true,
            });
        }
        Err(e) => {
            return Err(AppError::Database(format!(
                "读取 Antigravity gen_metadata 失败: {e}"
            )));
        }
    };
    tx.commit()
        .map_err(|e| AppError::Database(format!("无法提交 Antigravity 只读事务: {e}")))?;

    if gen_entries.is_empty() {
        if !step_states.has_running_step {
            let conn = lock_conn!(db.conn);
            update_sync_state_on_conn(&conn, &file_path_str, file_modified, last_gen_idx)?;
        }
        return Ok(SingleFileSync {
            imported: 0,
            skipped: 0,
            deferred: step_states.has_running_step,
        });
    }

    let session_id = trajectory_meta
        .as_ref()
        .and_then(|meta| meta.session_id.clone())
        .or_else(|| {
            db_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem.to_string())
        });
    let fallback_created_at = trajectory_meta
        .as_ref()
        .and_then(|meta| meta.created_at_seconds)
        .unwrap_or(file_modified_secs);

    let last_idx = gen_entries.last().map(|entry| entry.idx);
    let skip_last_as_unmapped_running = step_states.has_unmapped_running && last_idx.is_some();

    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut errors: Vec<String> = Vec::new();
    let mut next_checkpoint = last_gen_idx;
    let mut blocked = false;

    let conn = lock_conn!(db.conn);
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| AppError::Database(format!("无法开启用量写入事务: {e}")))?;

    for entry in &gen_entries {
        if entry.idx < last_gen_idx {
            continue;
        }

        let mapped_running = step_states.running_gen_idxs.contains(&entry.idx);
        let unmapped_tail = skip_last_as_unmapped_running && Some(entry.idx) == last_idx;
        if mapped_running || unmapped_tail {
            blocked = true;
            continue;
        }
        if entry.parse_failed {
            blocked = true;
            continue;
        }
        let Some(token_data) = &entry.token_data else {
            if !blocked {
                next_checkpoint = entry.idx + 1;
            }
            continue;
        };

        let session_id_str = session_id.as_deref().unwrap_or("unknown");
        let request_id = format!("{DATA_SOURCE}:{session_id_str}:{}", entry.idx);
        let created_at = step_states
            .by_gen_idx
            .get(&entry.idx)
            .and_then(|state| state.timestamp)
            .unwrap_or(fallback_created_at);

        match insert_antigravity_session_entry_on_conn(
            &tx,
            &request_id,
            token_data,
            session_id.as_deref(),
            created_at,
        ) {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                let msg = format!("插入失败 ({request_id}): {e}");
                log::warn!("[ANTIGRAVITY-SYNC] {msg}");
                errors.push(msg);
                skipped += 1;
                blocked = true;
                continue;
            }
        }
        if !blocked {
            next_checkpoint = entry.idx + 1;
        }
    }

    let deferred = blocked || step_states.has_running_step || !errors.is_empty();
    if next_checkpoint > last_gen_idx || !deferred {
        update_sync_state_on_conn(&tx, &file_path_str, file_modified, next_checkpoint)?;
    }

    if errors.is_empty() {
        tx.commit()
            .map_err(|e| AppError::Database(format!("提交 Antigravity 用量事务失败: {e}")))?;
    } else {
        tx.rollback()
            .map_err(|e| AppError::Database(format!("回滚 Antigravity 用量事务失败: {e}")))?;
        return Err(AppError::Database(errors.join("; ")));
    }

    Ok(SingleFileSync {
        imported,
        skipped,
        deferred,
    })
}

fn read_trajectory_metadata(conn: &rusqlite::Connection) -> Option<TrajectoryMetadata> {
    let mut stmt = conn
        .prepare("SELECT data FROM trajectory_metadata_blob WHERE id = 'main'")
        .ok()?;
    let data: Vec<u8> = stmt.query_row([], |row| row.get(0)).ok()?;
    let mut parser = ProtoParser::new(&data);
    let mut meta = TrajectoryMetadata::default();

    while let Some((field, value)) = parser.next_field() {
        match (field, value) {
            (2, ProtoValue::LengthDelimited(nested)) => {
                let mut timestamp = ProtoParser::new(nested);
                meta.created_at_seconds = timestamp.get_varint(1).map(|value| value as i64);
            }
            (3, ProtoValue::LengthDelimited(session_id_bytes)) => {
                meta.session_id = std::str::from_utf8(session_id_bytes)
                    .ok()
                    .map(|s| s.to_string());
            }
            _ => {}
        }
    }

    Some(meta)
}

fn read_gen_metadata_entries(
    conn: &rusqlite::Connection,
    last_gen_idx: i64,
) -> Result<Vec<GenMetadataEntry>, rusqlite::Error> {
    let mut entries = Vec::new();
    let mut stmt =
        conn.prepare("SELECT idx, data FROM gen_metadata WHERE idx >= ?1 ORDER BY idx")?;
    let rows = stmt.query_map(rusqlite::params![last_gen_idx], |row| {
        let idx: i64 = row.get(0)?;
        let data: Vec<u8> = row.get(1)?;
        Ok((idx, data))
    })?;

    for row in rows {
        let (idx, data) = row?;
        if data.len() > MAX_GEN_BLOB_BYTES {
            entries.push(GenMetadataEntry {
                idx,
                token_data: None,
                parse_failed: true,
            });
            continue;
        }
        entries.push(GenMetadataEntry {
            idx,
            parse_failed: false,
            token_data: parse_gen_metadata_blob(&data),
        });
    }

    Ok(entries)
}

fn read_step_states(conn: &rusqlite::Connection) -> Result<AntigravityStepStates, rusqlite::Error> {
    let mut by_gen_idx = HashMap::new();
    let mut running_gen_idxs = HashSet::new();
    let mut has_running_step = false;
    let mut has_unmapped_running = false;
    let mut stmt = conn.prepare("SELECT status, metadata FROM steps")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?))
    })?;
    for row in rows {
        let (status, metadata) = row?;
        let running = !is_terminal_status(status);
        if running {
            has_running_step = true;
        }
        let Some(metadata) = metadata else {
            if running {
                has_unmapped_running = true;
            }
            continue;
        };
        match parse_step_metadata(&metadata) {
            (Some(gen_idx), timestamp) => {
                by_gen_idx
                    .entry(gen_idx)
                    .and_modify(|state: &mut AntigravityStepState| {
                        if timestamp
                            .map(|ts| {
                                state
                                    .timestamp
                                    .map(|existing| ts < existing)
                                    .unwrap_or(true)
                            })
                            .unwrap_or(false)
                        {
                            state.timestamp = timestamp;
                        }
                        state.running |= running;
                    })
                    .or_insert(AntigravityStepState { timestamp, running });
                if running {
                    running_gen_idxs.insert(gen_idx);
                }
            }
            (None, _) if running => {
                has_unmapped_running = true;
            }
            _ => {}
        }
    }
    Ok(AntigravityStepStates {
        by_gen_idx,
        running_gen_idxs,
        has_running_step,
        has_unmapped_running,
    })
}

fn parse_step_metadata(data: &[u8]) -> (Option<i64>, Option<i64>) {
    let mut parser = ProtoParser::new(data);
    let mut timestamp: Option<i64> = None;
    let mut gen_idx: Option<i64> = None;

    while let Some((field, value)) = parser.next_field() {
        match (field, value) {
            (1, ProtoValue::LengthDelimited(nested)) => {
                let mut ts = ProtoParser::new(nested);
                timestamp = ts.get_varint(1).map(|value| value as i64);
            }
            (20, ProtoValue::LengthDelimited(nested)) => {
                let mut f20 = ProtoParser::new(nested);
                // f3 is the gen_metadata idx. idx=0 generations often omit it;
                // missing gen_idx is filled with 0 below when a timestamp exists.
                gen_idx = f20.get_varint(3).map(|value| value as i64);
            }
            _ => {}
        }
    }

    if gen_idx.is_none() && timestamp.is_some() {
        gen_idx = Some(0);
    }
    (gen_idx, timestamp)
}

fn parse_gen_metadata_blob(data: &[u8]) -> Option<AntigravityTokenData> {
    let mut parser = ProtoParser::new(data);
    let f1_blob = parser.get_nested(1)?;
    let mut f1 = ProtoParser::new(f1_blob);
    let mut step_tokens = AntigravityTokenData::default();
    let mut cumulative_tokens = AntigravityTokenData::default();
    let mut model = String::new();

    while let Some((field, value)) = f1.next_field() {
        match (field, value) {
            (4, ProtoValue::LengthDelimited(nested)) => {
                extract_token_fields(nested, &mut step_tokens);
            }
            (17, ProtoValue::LengthDelimited(nested)) => {
                let mut f17 = ProtoParser::new(nested);
                if let Some(f2_blob) = f17.get_nested(2) {
                    extract_token_fields(f2_blob, &mut cumulative_tokens);
                }
            }
            (19, ProtoValue::LengthDelimited(value_bytes)) => {
                if let Ok(value) = std::str::from_utf8(value_bytes) {
                    model = value.to_string();
                }
            }
            (20, ProtoValue::LengthDelimited(nested)) if model.is_empty() => {
                let mut tag_parser = ProtoParser::new(nested);
                let mut tag_key = None;
                let mut tag_value = None;
                while let Some((field, value)) = tag_parser.next_field() {
                    if let ProtoValue::LengthDelimited(val) = value {
                        match field {
                            1 => tag_key = std::str::from_utf8(val).ok().map(|s| s.to_string()),
                            2 => tag_value = std::str::from_utf8(val).ok().map(|s| s.to_string()),
                            _ => {}
                        }
                    }
                }
                if tag_key.as_deref() == Some("model_enum") {
                    if let Some(value) = tag_value {
                        model = value;
                    }
                }
            }
            _ => {}
        }
    }

    let mut token_data = if step_tokens.has_tokens() {
        step_tokens
    } else {
        cumulative_tokens
    };
    if !token_data.has_tokens() {
        return None;
    }
    if model.trim().is_empty() {
        model = "unknown".to_string();
    }
    token_data.model = model;
    Some(token_data)
}

fn clamp_tokens(value: u64) -> u32 {
    value.min(u32::MAX as u64) as u32
}

fn extract_token_fields(data: &[u8], tokens: &mut AntigravityTokenData) {
    let mut parser = ProtoParser::new(data);
    while let Some((field, value)) = parser.next_field() {
        let ProtoValue::Varint(value) = value else {
            continue;
        };
        match field {
            2 => tokens.input_tokens = clamp_tokens(value),
            3 => tokens.output_tokens = clamp_tokens(value),
            5 => tokens.cached_tokens = clamp_tokens(value),
            _ => {}
        }
    }
}

/// Map Antigravity placeholder / physical IDs onto billing model IDs already
/// seeded in `model_pricing`. Unknown `model_placeholder_*` (including live
/// M298/M318) stay `"unknown"` — do not guess them into a paid Gemini SKU.
fn resolve_antigravity_pricing_placeholder(normalized: &str) -> Option<String> {
    let without_thinking = normalized.strip_suffix("-thinking").unwrap_or(normalized);
    match without_thinking {
        "model_placeholder_m187" | "gemini-default" => Some("gemini-3.5-flash".to_string()),
        "model_placeholder_m20" => Some("gemini-3.5-flash".to_string()),
        "model_placeholder_m132" | "gemini-3-flash-a" => Some("gemini-3.5-flash".to_string()),
        "model_placeholder_m36" | "gemini-3.1-pro-low" => {
            Some("gemini-3.1-pro-preview".to_string())
        }
        "model_placeholder_m16" | "gemini-pro-default" => {
            Some("gemini-3.1-pro-preview".to_string())
        }
        "model_placeholder_m35" | "claude-sonnet-4-6" => {
            Some("claude-sonnet-4-6-20260217".to_string())
        }
        "model_placeholder_m26" | "claude-opus-4-6" => Some("claude-opus-4-6-20260206".to_string()),
        "gpt-oss-120b-medium" => Some("gpt-oss-120b-medium".to_string()),
        "unknown" | "null" | "none" | "" => Some("unknown".to_string()),
        other if other.starts_with("model_placeholder_") => Some("unknown".to_string()),
        _ => None,
    }
}

fn normalize_antigravity_pricing_model(raw_model: &str) -> String {
    let normalized = raw_model.trim().to_ascii_lowercase();
    if let Some(resolved) = resolve_antigravity_pricing_placeholder(&normalized) {
        return resolved;
    }

    let without_thinking = normalized
        .strip_suffix("-thinking")
        .unwrap_or(&normalized)
        .to_string();
    if let Some(base) = without_thinking.strip_suffix("-a") {
        return format!("{base}-preview");
    }
    if let Some(base) = without_thinking.strip_suffix("-b") {
        return format!("{base}-preview");
    }
    without_thinking
}

fn insert_antigravity_session_entry(
    db: &Database,
    request_id: &str,
    token_data: &AntigravityTokenData,
    session_id: Option<&str>,
    created_at: i64,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);
    insert_antigravity_session_entry_on_conn(&conn, request_id, token_data, session_id, created_at)
}

fn insert_antigravity_session_entry_on_conn(
    conn: &rusqlite::Connection,
    request_id: &str,
    token_data: &AntigravityTokenData,
    session_id: Option<&str>,
    created_at: i64,
) -> Result<bool, AppError> {
    let output_tokens = token_data.output_tokens;
    let input_tokens = token_data.input_tokens;
    let raw_model = token_data.model.trim();
    let model = if raw_model.is_empty() {
        "unknown"
    } else {
        raw_model
    };
    let pricing_model = normalize_antigravity_pricing_model(model);

    let usage = TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens: token_data.cached_tokens,
        cache_creation_tokens: 0,
        model: Some(pricing_model.clone()),
        message_id: None,
    };

    let pricing = find_model_pricing(conn, &pricing_model);
    if pricing.is_none() && !is_placeholder_pricing_model(&pricing_model) {
        log::warn!("[ANTIGRAVITY-SYNC] 模型未命中定价: {model} -> {pricing_model}");
    }

    let multiplier = Decimal::from(1);
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing
    {
        Some(pricing) => {
            let cost = CostCalculator::calculate_for_app(APP_TYPE, &usage, &pricing, multiplier);
            (
                cost.input_cost.to_string(),
                cost.output_cost.to_string(),
                cost.cache_read_cost.to_string(),
                cost.cache_creation_cost.to_string(),
                cost.total_cost.to_string(),
            )
        }
        None => (
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
        ),
    };

    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model, pricing_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source,
            input_token_semantics
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)
        ON CONFLICT(request_id) DO UPDATE SET
            model = excluded.model,
            request_model = excluded.request_model,
            pricing_model = excluded.pricing_model,
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            input_cost_usd = excluded.input_cost_usd,
            output_cost_usd = excluded.output_cost_usd,
            cache_read_cost_usd = excluded.cache_read_cost_usd,
            cache_creation_cost_usd = excluded.cache_creation_cost_usd,
            total_cost_usd = excluded.total_cost_usd
        WHERE data_source = 'antigravity_session'
          AND (input_tokens != excluded.input_tokens
           OR output_tokens != excluded.output_tokens
           OR cache_read_tokens != excluded.cache_read_tokens
           OR model != excluded.model
           OR COALESCE(pricing_model, '') != COALESCE(excluded.pricing_model, ''))",
        rusqlite::params![
            request_id,
            PROVIDER_ID,
            APP_TYPE,
            model,
            model,
            pricing_model,
            input_tokens,
            output_tokens,
            token_data.cached_tokens,
            0i64,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            0i64,
            Option::<i64>::None,
            200i64,
            Option::<String>::None,
            session_id.map(|value| value.to_string()),
            Some(DATA_SOURCE),
            1i64,
            "1.0",
            created_at,
            DATA_SOURCE,
            INPUT_TOKEN_SEMANTICS_FRESH,
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 Antigravity 会话日志失败: {e}")))?;

    Ok(conn.changes() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::session_usage::get_sync_state;
    use std::time::Duration;

    #[test]
    fn test_collect_antigravity_db_files_nonexistent() {
        let files = collect_antigravity_db_files(Path::new("/nonexistent/path"));
        assert!(files.is_empty());
    }

    fn proto_varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                break;
            }
        }
        out
    }

    fn proto_varint_field(field: u32, value: u64) -> Vec<u8> {
        let mut out = proto_varint((field as u64) << 3);
        out.extend(proto_varint(value));
        out
    }

    fn proto_len_field(field: u32, payload: Vec<u8>) -> Vec<u8> {
        let mut out = proto_varint(((field as u64) << 3) | 2);
        out.extend(proto_varint(payload.len() as u64));
        out.extend(payload);
        out
    }

    fn antigravity_gen_metadata(
        input: u64,
        output: u64,
        cache_read: u64,
        non_thinking_output: u64,
        thinking_output: u64,
    ) -> Vec<u8> {
        let mut usage = proto_varint_field(1, 1016);
        usage.extend(proto_varint_field(2, input));
        usage.extend(proto_varint_field(3, output));
        usage.extend(proto_varint_field(5, cache_read));
        usage.extend(proto_varint_field(9, non_thinking_output));
        usage.extend(proto_varint_field(10, thinking_output));

        let mut metadata = proto_len_field(4, usage);
        metadata.extend(proto_len_field(19, b"gemini-3-flash-a".to_vec()));
        proto_len_field(1, metadata)
    }

    #[test]
    fn test_parse_antigravity_usage_uses_f2_f3_f5_only() {
        let data = antigravity_gen_metadata(8_741, 14_479, 28_519, 12_672, 1_807);

        let usage = parse_gen_metadata_blob(&data).expect("Agy usage should parse");
        assert_eq!(usage.input_tokens, 8_741);
        assert_eq!(usage.output_tokens, 14_479);
        assert_eq!(usage.cached_tokens, 28_519);
        assert_eq!(usage.model, "gemini-3-flash-a");
    }

    fn step_metadata(gen_idx: i64, timestamp: i64) -> Vec<u8> {
        let ts = proto_varint_field(1, timestamp as u64);
        let gen_ref = proto_varint_field(3, gen_idx as u64);
        let mut out = proto_len_field(1, ts);
        out.extend(proto_len_field(20, gen_ref));
        out
    }

    fn step_metadata_idx0_timestamp_only(timestamp: i64) -> Vec<u8> {
        proto_len_field(1, proto_varint_field(1, timestamp as u64))
    }

    fn write_agy_fixture(
        db_path: &Path,
        gens: &[(i64, Vec<u8>)],
        steps: &[(i64, Vec<u8>)],
    ) -> Result<(), AppError> {
        let conn = rusqlite::Connection::open(db_path)
            .map_err(|e| AppError::Database(format!("open fixture db: {e}")))?;
        conn.execute_batch(
            "CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB, size INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE steps (status INTEGER NOT NULL DEFAULT 0, metadata BLOB);",
        )
        .map_err(|e| AppError::Database(format!("create fixture schema: {e}")))?;
        for (idx, data) in gens {
            conn.execute(
                "INSERT INTO gen_metadata (idx, data, size) VALUES (?1, ?2, ?3)",
                rusqlite::params![idx, data, data.len() as i64],
            )
            .map_err(|e| AppError::Database(format!("insert fixture gen: {e}")))?;
        }
        for (status, metadata) in steps {
            conn.execute(
                "INSERT INTO steps VALUES (?1, ?2)",
                rusqlite::params![status, metadata],
            )
            .map_err(|e| AppError::Database(format!("insert fixture step: {e}")))?;
        }
        Ok(())
    }

    #[test]
    fn composite_mtime_ignores_shm() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("agy.db");
        fs::write(&db_path, b"db").map_err(|e| AppError::Config(format!("write db: {e}")))?;
        let db_mtime = composite_modified_nanos(&db_path)?;
        std::thread::sleep(Duration::from_millis(30));
        fs::write(format!("{}-shm", db_path.to_string_lossy()), b"shm")
            .map_err(|e| AppError::Config(format!("write shm: {e}")))?;
        assert_eq!(composite_modified_nanos(&db_path)?, db_mtime);
        Ok(())
    }

    #[test]
    fn test_read_step_states_non_terminal_protects_running() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE steps (status INTEGER, metadata BLOB);")
            .unwrap();
        conn.execute(
            "INSERT INTO steps VALUES (?1, ?2), (?3, ?4)",
            rusqlite::params![6, step_metadata(1, 100), 7, step_metadata(2, 200)],
        )
        .unwrap();

        let states = read_step_states(&conn).expect("read step states");
        assert!(!states.has_running_step);
        assert_eq!(
            states.by_gen_idx.get(&1).and_then(|s| s.timestamp),
            Some(100)
        );

        conn.execute(
            "INSERT INTO steps VALUES (?1, ?2)",
            rusqlite::params![ANTIGRAVITY_STATUS_GENERATING, step_metadata(3, 300)],
        )
        .unwrap();
        let states = read_step_states(&conn).expect("read step states");
        assert!(states.has_running_step);
        assert!(states.running_gen_idxs.contains(&3));
    }

    #[test]
    fn missing_f20_f3_maps_timestamp_to_gen_idx_zero() {
        let (gen_idx, ts) = parse_step_metadata(&step_metadata_idx0_timestamp_only(42));
        assert_eq!(gen_idx, Some(0));
        assert_eq!(ts, Some(42));
    }

    #[test]
    fn test_sync_antigravity_db_does_not_advance_while_running() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("agy.db");
        write_agy_fixture(
            &db_path,
            &[],
            &[(ANTIGRAVITY_STATUS_GENERATING, step_metadata(1, 100))],
        )?;

        let db = Database::memory()?;
        let result = sync_single_antigravity_db(&db, &db_path, 0, 0)?;
        assert_eq!((result.imported, result.skipped), (0, 0));
        assert!(result.deferred);

        let key = db_path.to_string_lossy().to_string();
        let (last_modified, last_offset) = get_sync_state(&db, &key)?;
        assert_eq!((last_modified, last_offset), (0, 0));
        Ok(())
    }

    #[test]
    fn test_sync_skips_in_flight_gen_with_tokens() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("agy.db");
        let blob = antigravity_gen_metadata(100, 10, 5, 8, 2);
        write_agy_fixture(
            &db_path,
            &[(1, blob)],
            &[(ANTIGRAVITY_STATUS_GENERATING, step_metadata(1, 100))],
        )?;

        let db = Database::memory()?;
        let result = sync_single_antigravity_db(&db, &db_path, 0, 0)?;
        assert_eq!(result.imported, 0);
        assert!(result.deferred);
        let key = db_path.to_string_lossy().to_string();
        assert_eq!(get_sync_state(&db, &key)?, (0, 0));

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'antigravity_session'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 0);
        Ok(())
    }

    #[test]
    fn test_sync_advances_completed_prefix_while_later_gen_runs() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("agy.db");
        let done = antigravity_gen_metadata(100, 10, 0, 10, 0);
        let running = antigravity_gen_metadata(50, 5, 20, 5, 0);
        write_agy_fixture(
            &db_path,
            &[(0, done), (1, running)],
            &[
                (3, step_metadata(0, 100)),
                (ANTIGRAVITY_STATUS_GENERATING, step_metadata(1, 200)),
            ],
        )?;

        let db = Database::memory()?;
        let result = sync_single_antigravity_db(&db, &db_path, 0, 0)?;
        assert_eq!(result.imported, 1);
        assert!(result.deferred);
        let key = db_path.to_string_lossy().to_string();
        let (last_modified, last_offset) = get_sync_state(&db, &key)?;
        assert!(last_modified > 0);
        assert_eq!(last_offset, 1);
        Ok(())
    }

    #[test]
    fn test_sync_antigravity_db_does_not_checkpoint_when_step_read_fails() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("agy.db");
        {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| AppError::Database(format!("open fixture db: {e}")))?;
            conn.execute_batch(
                "CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB, size INTEGER NOT NULL DEFAULT 0);",
            )
            .map_err(|e| AppError::Database(format!("create fixture schema: {e}")))?;
        }

        let db = Database::memory()?;
        assert!(sync_single_antigravity_db(&db, &db_path, 0, 0).is_err());

        let key = db_path.to_string_lossy().to_string();
        let (last_modified, last_offset) = get_sync_state(&db, &key)?;
        assert_eq!((last_modified, last_offset), (0, 0));
        Ok(())
    }

    #[test]
    fn test_sync_antigravity_db_advances_for_canceled_or_failed_steps() -> Result<(), AppError> {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("agy.db");
        write_agy_fixture(
            &db_path,
            &[],
            &[(6, step_metadata(1, 100)), (7, step_metadata(2, 200))],
        )?;

        let db = Database::memory()?;
        let result = sync_single_antigravity_db(&db, &db_path, 0, 0)?;
        assert_eq!((result.imported, result.skipped), (0, 0));
        assert!(!result.deferred);

        let key = db_path.to_string_lossy().to_string();
        let (last_modified, last_offset) = get_sync_state(&db, &key)?;
        assert!(last_modified > 0);
        assert_eq!(last_offset, 0);
        Ok(())
    }

    #[test]
    fn test_insert_antigravity_session_entry_upserts_existing_request() -> Result<(), AppError> {
        let db = Database::memory()?;
        let request_id = "antigravity_session:upsert:1";
        let first = AntigravityTokenData {
            input_tokens: 10,
            output_tokens: 2,
            cached_tokens: 1,
            model: "gemini-3-pro-b".to_string(),
        };
        assert!(insert_antigravity_session_entry(
            &db,
            request_id,
            &first,
            Some("agy-session"),
            1000,
        )?);

        let second = AntigravityTokenData {
            input_tokens: 20,
            output_tokens: 7,
            cached_tokens: 2,
            model: "gemini-3-flash-a-thinking".to_string(),
        };
        assert!(insert_antigravity_session_entry(
            &db,
            request_id,
            &second,
            Some("agy-session"),
            2000,
        )?);

        let conn = lock_conn!(db.conn);
        let row: (i64, i64, i64, String, String, String, i64, i64) = conn.query_row(
            "SELECT input_tokens, output_tokens, cache_read_tokens, model, pricing_model, app_type, created_at, input_token_semantics
             FROM proxy_request_logs WHERE request_id = ?1",
            rusqlite::params![request_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )?;
        assert_eq!(
            row,
            (
                20,
                7,
                2,
                "gemini-3-flash-a-thinking".to_string(),
                "gemini-3.5-flash".to_string(),
                APP_TYPE.to_string(),
                1000,
                INPUT_TOKEN_SEMANTICS_FRESH,
            )
        );

        Ok(())
    }

    #[test]
    fn inserts_antigravity_usage_with_fresh_input_cost_semantics() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO model_pricing (
                    model_id, display_name, input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
                ) VALUES ('antigravity-test-model', 'Antigravity Test', '1', '10', '0.1', '0')",
                [],
            )?;
        }

        let usage = AntigravityTokenData {
            input_tokens: 1000,
            output_tokens: 100,
            cached_tokens: 500,
            model: "antigravity-test-model".to_string(),
        };
        let inserted = insert_antigravity_session_entry(
            &db,
            "antigravity-test",
            &usage,
            Some("antigravity:test"),
            1_779_237_991,
        )?;
        assert!(inserted);

        let conn = lock_conn!(db.conn);
        let (app_type, pricing_model, input_cost, cache_read_cost, total_cost): (
            String,
            Option<String>,
            String,
            String,
            String,
        ) = conn.query_row(
            "SELECT app_type, pricing_model, input_cost_usd, cache_read_cost_usd, total_cost_usd
                 FROM proxy_request_logs WHERE request_id = 'antigravity-test'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
        assert_eq!(app_type, APP_TYPE);
        assert_eq!(pricing_model.as_deref(), Some("antigravity-test-model"));
        assert_eq!(input_cost, "0.001");
        assert_eq!(cache_read_cost, "0.00005");
        assert_eq!(total_cost, "0.00205");

        Ok(())
    }

    #[test]
    fn gemini_3_8_flash_is_seeded_and_priced() -> Result<(), AppError> {
        let db = Database::memory()?;
        let usage = AntigravityTokenData {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cached_tokens: 1_000_000,
            model: "gemini-3.8-flash".to_string(),
        };
        assert!(insert_antigravity_session_entry(
            &db,
            "antigravity-38",
            &usage,
            Some("s"),
            1,
        )?);
        let conn = lock_conn!(db.conn);
        let total: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id = 'antigravity-38'",
            [],
            |row| row.get(0),
        )?;
        assert_ne!(total, "0");
        assert_ne!(total, "0.0");
        Ok(())
    }

    #[test]
    fn test_normalize_antigravity_pricing_model_aliases() {
        assert_eq!(
            normalize_antigravity_pricing_model("gemini-3-flash-a-thinking"),
            "gemini-3.5-flash"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("gemini-3-pro-b"),
            "gemini-3-pro-preview"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("gemini-pro-default"),
            "gemini-3.1-pro-preview"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("MODEL_PLACEHOLDER_M298"),
            "unknown"
        );
        assert_eq!(
            normalize_antigravity_pricing_model("MODEL_PLACEHOLDER_M318"),
            "unknown"
        );
        assert_eq!(normalize_antigravity_pricing_model("unknown"), "unknown");
        assert_eq!(
            normalize_antigravity_pricing_model("gemini-3.8-flash"),
            "gemini-3.8-flash"
        );
    }
}
