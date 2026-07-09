import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { tpsApi, type TpsFilters } from "@/lib/api/tps";

/**
 * TPS 监控 & 记录 —— React Query 键 & hooks（本地定制，fork 专用）
 *
 * 与 usage 查询键完全独立，互不干扰。
 */

const DEFAULT_REFETCH_INTERVAL_MS = 30_000;

/** 把过滤对象压成稳定的原始值数组，作为 query key 的一部分。 */
function filtersKey(f: TpsFilters) {
  return [
    f.appType ?? null,
    f.model ?? null,
    f.providerId ?? null,
    f.startDate ?? null,
    f.endDate ?? null,
  ] as const;
}

export const tpsKeys = {
  all: ["tps"] as const,
  summary: (filters: TpsFilters) =>
    [...tpsKeys.all, "summary", ...filtersKey(filters)] as const,
  recent: (filters: TpsFilters, limit: number) =>
    [...tpsKeys.all, "recent", ...filtersKey(filters), limit] as const,
  trend: (filters: TpsFilters, buckets: number) =>
    [...tpsKeys.all, "trend", ...filtersKey(filters), buckets] as const,
  count: () => [...tpsKeys.all, "count"] as const,
  enabled: () => [...tpsKeys.all, "enabled"] as const,
};

type TpsQueryOptions = {
  refetchInterval?: number | false;
  refetchIntervalInBackground?: boolean;
  enabled?: boolean;
};

export function useTpsSummary(
  filters: TpsFilters = {},
  options: TpsQueryOptions = {},
) {
  return useQuery({
    queryKey: tpsKeys.summary(filters),
    queryFn: () => tpsApi.getSummary(filters),
    refetchInterval: options.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options.refetchIntervalInBackground ?? false,
    enabled: options.enabled ?? true,
  });
}

export function useTpsRecent(
  filters: TpsFilters = {},
  limit = 100,
  options: TpsQueryOptions = {},
) {
  return useQuery({
    queryKey: tpsKeys.recent(filters, limit),
    queryFn: () => tpsApi.getRecent(filters, limit),
    refetchInterval: options.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options.refetchIntervalInBackground ?? false,
    enabled: options.enabled ?? true,
  });
}

export function useTpsTrend(
  filters: TpsFilters = {},
  buckets = 48,
  options: TpsQueryOptions = {},
) {
  return useQuery({
    queryKey: tpsKeys.trend(filters, buckets),
    queryFn: () => tpsApi.getTrend(filters, buckets),
    refetchInterval: options.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options.refetchIntervalInBackground ?? false,
    enabled: options.enabled ?? true,
  });
}

export function useTpsCount(options: TpsQueryOptions = {}) {
  return useQuery({
    queryKey: tpsKeys.count(),
    queryFn: () => tpsApi.countSamples(),
    refetchInterval: options.refetchInterval ?? DEFAULT_REFETCH_INTERVAL_MS,
    refetchIntervalInBackground: options.refetchIntervalInBackground ?? false,
  });
}

export function useTpsEnabled() {
  return useQuery({
    queryKey: tpsKeys.enabled(),
    queryFn: () => tpsApi.getEnabled(),
    // 开关变更由 mutation 主动 invalidate，无需频繁轮询。
    refetchInterval: false,
  });
}

export function useSetTpsEnabled() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (enabled: boolean) => tpsApi.setEnabled(enabled),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: tpsKeys.enabled() });
      queryClient.invalidateQueries({ queryKey: tpsKeys.all });
    },
  });
}

export function useClearTpsSamples() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (filters?: TpsFilters) => tpsApi.clearSamples(filters),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: tpsKeys.all });
    },
  });
}
