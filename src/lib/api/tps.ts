import { invoke } from "@tauri-apps/api/core";

/**
 * TPS 监控 & 记录 —— 前端 API 层（本地定制，fork 专用）
 *
 * 与 usage API 解耦：所有调用走独立的 `tps_*` Tauri 命令。
 * 类型与后端 `src-tauri/src/services/tps.rs` 的 serde(camelCase) 结构对齐。
 */

export interface TpsSample {
  id: number;
  requestId: string;
  appType: string;
  providerId: string;
  model: string;
  outputTokens: number;
  firstTokenMs: number | null;
  durationMs: number | null;
  isStreaming: boolean;
  tps: number;
  createdAt: number;
}

export interface TpsSummary {
  sampleCount: number;
  streamingCount: number;
  nonStreamingCount: number;
  totalOutputTokens: number;
  avgTps: number;
  maxTps: number;
  minTps: number;
  p50Tps: number;
  p95Tps: number;
}

export interface TpsTrendPoint {
  bucket: number;
  avgTps: number;
  sampleCount: number;
  totalOutputTokens: number;
}

export interface TpsFilters {
  appType?: string;
  model?: string;
  providerId?: string;
  startDate?: number;
  endDate?: number;
}

/** 分组维度：按 Provider 或 Model 拆分 TPS */
export type TpsGroupBy = "provider" | "model";

/** 按分组聚合的 TPS 统计（每个 provider 或 model 一行） */
export interface TpsGroupStats {
  /** 分组键：Provider 时为 provider_id，Model 时为 model 名 */
  key: string;
  /** 展示名：Provider 时优先取写入时冗余的 provider_name，其次实时 providers.name（缺失则回退 provider_id），Model 时为 null */
  displayName: string | null;
  /** 仅 Provider 分组有值：该 provider 所属 app_type */
  appType: string | null;
  /** 仅 Provider 分组有值：该 provider 是否已删除（前端据此做删除视觉）。Model 分组为 null */
  isDeleted?: boolean | null;
  sampleCount: number;
  avgTps: number;
  maxTps: number;
  p95Tps: number;
  totalOutputTokens: number;
}

/** 并发采样点 */
export interface ConcurrencySample {
  /** unix 秒 */
  ts: number;
  /** 当时在途并发数 */
  count: number;
}

/** 并发快照 */
export interface ConcurrencySnapshot {
  current: number;
  peak: number;
  history: ConcurrencySample[];
}

export const tpsApi = {
  getSummary: async (filters: TpsFilters = {}): Promise<TpsSummary> =>
    invoke<TpsSummary>("get_tps_summary", { filters }),

  getRecent: async (
    filters: TpsFilters = {},
    limit = 100,
  ): Promise<TpsSample[]> =>
    invoke<TpsSample[]>("get_tps_recent", { filters, limit }),

  getTrend: async (
    filters: TpsFilters = {},
    buckets = 48,
  ): Promise<TpsTrendPoint[]> =>
    invoke<TpsTrendPoint[]>("get_tps_trend", { filters, buckets }),

  getBreakdown: async (
    filters: TpsFilters = {},
    groupBy: TpsGroupBy,
  ): Promise<TpsGroupStats[]> =>
    invoke<TpsGroupStats[]>("get_tps_breakdown", { filters, groupBy }),

  clearSamples: async (filters?: TpsFilters): Promise<number> =>
    invoke<number>("clear_tps_samples", { filters }),

  countSamples: async (): Promise<number> =>
    invoke<number>("count_tps_samples"),

  getEnabled: async (): Promise<boolean> => invoke<boolean>("get_tps_enabled"),

  setEnabled: async (enabled: boolean): Promise<void> =>
    invoke<void>("set_tps_enabled", { enabled }),

  // 并发监控（独立特性）
  getConcurrency: async (): Promise<ConcurrencySnapshot> =>
    invoke<ConcurrencySnapshot>("get_concurrency"),

  resetConcurrencyPeak: async (): Promise<void> =>
    invoke<void>("reset_concurrency_peak"),
};
