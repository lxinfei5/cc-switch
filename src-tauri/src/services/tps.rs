//! TPS 监控 & 记录 —— 数据访问层
//!
//! 本模块为本地定制（fork 专用），与 `services/usage_stats.rs`（成本核算）解耦：
//! TPS 属于「可观测性」而非「计费」，数据独立落在 `tps_samples` 表，
//! 删除/清空不影响 `proxy_request_logs`。
//!
//! TPS 定义：output_tokens / generation_seconds。
//! - 流式且有 first_token_ms：generation = duration_ms - first_token_ms（首 token 之后的纯生成耗时）
//! - 否则：generation = duration_ms（或 latency 兜底）
//! 在写入侧（`tps::on_request_logged`）计算好 `tps` 后落库，查询侧只读。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use serde::{Deserialize, Serialize};

/// 写入时传入的单条样本（tps 已在调用侧算好）
#[derive(Debug, Clone)]
pub struct TpsSampleInput {
    pub request_id: String,
    pub app_type: String,
    pub provider_id: String,
    pub model: String,
    pub output_tokens: i64,
    pub first_token_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub is_streaming: bool,
    pub tps: f64,
    pub created_at: i64,
}

/// 返回给前端的单条样本
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TpsSample {
    pub id: i64,
    pub request_id: String,
    pub app_type: String,
    pub provider_id: String,
    pub model: String,
    pub output_tokens: i64,
    pub first_token_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub is_streaming: bool,
    pub tps: f64,
    pub created_at: i64,
}

/// TPS 汇总指标
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TpsSummary {
    pub sample_count: u64,
    pub streaming_count: u64,
    pub non_streaming_count: u64,
    pub total_output_tokens: u64,
    /// 仅统计 tps > 0 的样本的平均值（避免 0 拖低均值）
    pub avg_tps: f64,
    pub max_tps: f64,
    pub min_tps: f64,
    pub p50_tps: f64,
    pub p95_tps: f64,
}

/// TPS 趋势的一个时间桶
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TpsTrendPoint {
    /// 桶起始的 unix 秒
    pub bucket: i64,
    pub avg_tps: f64,
    pub sample_count: u64,
    pub total_output_tokens: u64,
}

/// 通用过滤参数（None = 不过滤）
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TpsFilters {
    pub app_type: Option<String>,
    pub model: Option<String>,
    pub provider_id: Option<String>,
    pub start_date: Option<i64>,
    pub end_date: Option<i64>,
}

/// 构造过滤条件列表与参数。`extra` 用于追加额外条件（如 `tps > 0`）。
fn build_conditions(
    filters: &TpsFilters,
    extra: &[&str],
) -> (Vec<String>, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut conds: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = filters.start_date {
        conds.push("created_at >= ?".to_string());
        params.push(Box::new(start));
    }
    if let Some(end) = filters.end_date {
        conds.push("created_at <= ?".to_string());
        params.push(Box::new(end));
    }
    if let Some(at) = &filters.app_type {
        conds.push("app_type = ?".to_string());
        params.push(Box::new(at.clone()));
    }
    if let Some(m) = &filters.model {
        conds.push("model = ?".to_string());
        params.push(Box::new(m.clone()));
    }
    if let Some(pid) = &filters.provider_id {
        conds.push("provider_id = ?".to_string());
        params.push(Box::new(pid.clone()));
    }

    (conds, params)
}

/// 把条件列表拼成 `WHERE ...`（空则返回空串）
fn where_from(conds: &[String]) -> String {
    if conds.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conds.join(" AND "))
    }
}

/// 百分位数（线性插值）。空切片返回 0.0。
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = (p / 100.0) * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        return sorted[lo.min(sorted.len() - 1)];
    }
    let frac = rank - lo as f64;
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

impl Database {
    /// 写入一条 TPS 样本（由 `tps::on_request_logged` 调用）
    pub fn insert_tps_sample(&self, input: &TpsSampleInput) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT INTO tps_samples (
                request_id, app_type, provider_id, model,
                output_tokens, first_token_ms, duration_ms,
                is_streaming, tps, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                input.request_id,
                input.app_type,
                input.provider_id,
                input.model,
                input.output_tokens,
                input.first_token_ms,
                input.duration_ms,
                input.is_streaming,
                input.tps,
                input.created_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("写入 tps_samples 失败: {e}")))?;
        Ok(())
    }

    /// TPS 汇总（含 p50/p95）
    pub fn get_tps_summary(&self, filters: &TpsFilters) -> Result<TpsSummary, AppError> {
        let conn = lock_conn!(self.conn);
        let (conds, params) = build_conditions(filters, &[]);
        let where_clause = where_from(&conds);

        // 聚合查询：count / streaming / non-streaming / total output / avg / max / min
        let sql = format!(
            "SELECT
                COUNT(*),
                COALESCE(SUM(CASE WHEN is_streaming = 1 THEN 1 ELSE 0 END), 0),
                COUNT(*) - COALESCE(SUM(CASE WHEN is_streaming = 1 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(AVG(CASE WHEN tps > 0 THEN tps END), 0),
                COALESCE(MAX(tps), 0),
                COALESCE(MIN(CASE WHEN tps > 0 THEN tps END), 0)
             FROM tps_samples {where_clause}"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(format!("准备 TPS 汇总查询失败: {e}")))?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let row = stmt
            .query_row(param_refs.as_slice(), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<f64>>(4)?,
                    r.get::<_, Option<f64>>(5)?,
                    r.get::<_, Option<f64>>(6)?,
                ))
            })
            .map_err(|e| AppError::Database(format!("执行 TPS 汇总查询失败: {e}")))?;
        drop(stmt);

        // 百分位数：拉取 tps>0 的样本排序后计算
        let (pct_conds, pct_params) = build_conditions(filters, &["tps > 0"]);
        let pct_where = where_from(&pct_conds);
        let pct_sql = format!("SELECT tps FROM tps_samples {pct_where} ORDER BY tps");
        let mut stmt2 = conn
            .prepare(&pct_sql)
            .map_err(|e| AppError::Database(format!("准备 TPS 百分位查询失败: {e}")))?;
        let pct_refs: Vec<&dyn rusqlite::ToSql> = pct_params.iter().map(|b| b.as_ref()).collect();
        let mut values: Vec<f64> = Vec::new();
        let mut rows = stmt2
            .query(pct_refs.as_slice())
            .map_err(|e| AppError::Database(format!("执行 TPS 百分位查询失败: {e}")))?;
        while let Some(r) = rows
            .next()
            .map_err(|e| AppError::Database(format!("读取 TPS 百分位行失败: {e}")))?
        {
            values.push(r.get::<_, f64>(0)?);
        }

        Ok(TpsSummary {
            sample_count: row.0 as u64,
            streaming_count: row.1 as u64,
            non_streaming_count: row.2 as u64,
            total_output_tokens: row.3 as u64,
            avg_tps: row.4.unwrap_or(0.0),
            max_tps: row.5.unwrap_or(0.0),
            min_tps: row.6.unwrap_or(0.0),
            p50_tps: percentile(&values, 50.0),
            p95_tps: percentile(&values, 95.0),
        })
    }

    /// 最近 N 条样本（倒序）
    pub fn get_tps_recent(
        &self,
        filters: &TpsFilters,
        limit: u32,
    ) -> Result<Vec<TpsSample>, AppError> {
        let conn = lock_conn!(self.conn);
        let (conds, params) = build_conditions(filters, &[]);
        let where_clause = where_from(&conds);
        let limit = limit.clamp(1, 500) as i64;
        let sql = format!(
            "SELECT id, request_id, app_type, provider_id, model,
                    output_tokens, first_token_ms, duration_ms, is_streaming, tps, created_at
             FROM tps_samples {where_clause}
             ORDER BY created_at DESC
             LIMIT {limit}"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(format!("准备 TPS 最近样本查询失败: {e}")))?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let mut rows = stmt
            .query(param_refs.as_slice())
            .map_err(|e| AppError::Database(format!("执行 TPS 最近样本查询失败: {e}")))?;
        let mut out = Vec::new();
        while let Some(r) = rows
            .next()
            .map_err(|e| AppError::Database(format!("读取 TPS 样本行失败: {e}")))?
        {
            out.push(TpsSample {
                id: r.get(0)?,
                request_id: r.get(1)?,
                app_type: r.get(2)?,
                provider_id: r.get(3)?,
                model: r.get(4)?,
                output_tokens: r.get(5)?,
                first_token_ms: r.get(6)?,
                duration_ms: r.get(7)?,
                is_streaming: r.get::<_, i64>(8)? != 0,
                tps: r.get(9)?,
                created_at: r.get(10)?,
            });
        }
        Ok(out)
    }

    /// 分桶趋势。`buckets` 为期望桶数；桶宽 = max(1, (end-start)/buckets) 秒。
    pub fn get_tps_trend(
        &self,
        filters: &TpsFilters,
        buckets: u32,
    ) -> Result<Vec<TpsTrendPoint>, AppError> {
        let conn = lock_conn!(self.conn);
        let start = filters.start_date.unwrap_or(0);
        let end = filters
            .end_date
            .unwrap_or_else(|| chrono::Utc::now().timestamp());
        let desired = buckets.clamp(1, 200) as i64;
        let span = (end - start).max(1);
        let bucket_sec = (span / desired).max(1);

        let (conds, params) = build_conditions(filters, &[]);
        let where_clause = where_from(&conds);
        let sql = format!(
            "SELECT
                (created_at / {bucket_sec}) * {bucket_sec} AS bucket,
                AVG(CASE WHEN tps > 0 THEN tps END),
                COUNT(*),
                COALESCE(SUM(output_tokens), 0)
             FROM tps_samples {where_clause}
             GROUP BY bucket
             ORDER BY bucket"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(format!("准备 TPS 趋势查询失败: {e}")))?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let mut rows = stmt
            .query(param_refs.as_slice())
            .map_err(|e| AppError::Database(format!("执行 TPS 趋势查询失败: {e}")))?;
        let mut out = Vec::new();
        while let Some(r) = rows
            .next()
            .map_err(|e| AppError::Database(format!("读取 TPS 趋势行失败: {e}")))?
        {
            out.push(TpsTrendPoint {
                bucket: r.get(0)?,
                avg_tps: r.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                sample_count: r.get::<_, i64>(2)? as u64,
                total_output_tokens: r.get::<_, i64>(3)? as u64,
            });
        }
        Ok(out)
    }

    /// 清空 TPS 样本（按过滤条件；过滤为空则清空全部）。返回删除行数。
    pub fn clear_tps_samples(&self, filters: &TpsFilters) -> Result<u64, AppError> {
        let conn = lock_conn!(self.conn);
        let (conds, params) = build_conditions(filters, &[]);
        let where_clause = where_from(&conds);
        let sql = format!("DELETE FROM tps_samples {where_clause}");
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let changed = conn
            .execute(&sql, param_refs.as_slice())
            .map_err(|e| AppError::Database(format!("清空 tps_samples 失败: {e}")))?;
        Ok(changed as u64)
    }

    /// TPS 样本总条数（供前端展示，独立于 summary 的时间范围）
    pub fn count_tps_samples(&self) -> Result<u64, AppError> {
        let conn = lock_conn!(self.conn);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tps_samples", [], |r| r.get(0))
            .map_err(|e| AppError::Database(format!("统计 tps_samples 失败: {e}")))?;
        Ok(count as u64)
    }
}