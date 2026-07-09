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

  clearSamples: async (filters?: TpsFilters): Promise<number> =>
    invoke<number>("clear_tps_samples", { filters }),

  countSamples: async (): Promise<number> =>
    invoke<number>("count_tps_samples"),

  getEnabled: async (): Promise<boolean> => invoke<boolean>("get_tps_enabled"),

  setEnabled: async (enabled: boolean): Promise<void> =>
    invoke<void>("set_tps_enabled", { enabled }),
};
