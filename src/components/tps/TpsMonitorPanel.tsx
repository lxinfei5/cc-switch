import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Activity, Gauge, Trash2, Zap, RotateCcw, Users } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  useTpsSummary,
  useTpsRecent,
  useTpsTrend,
  useTpsBreakdown,
  useTpsEnabled,
  useSetTpsEnabled,
  useClearTpsSamples,
  useConcurrency,
  useResetConcurrencyPeak,
} from "@/lib/query/tps";
import { useTpsEventBridge } from "@/hooks/useTpsEventBridge";
import type { TpsGroupStats, TpsSample, TpsTrendPoint } from "@/lib/api/tps";
import { cn } from "@/lib/utils";

/**
 * TPS 监控 & 记录 面板（本地定制，fork 专用）
 *
 * 自包含：所有数据通过 `@/lib/query/tps` 的 hook 拉取，事件通过
 * `useTpsEventBridge` 实时刷新。移除本组件只需删掉 App.tsx 里的 view 分支与
 * nav 按钮，不影响其它功能。
 *
 * 区块：并发（实时）→ 时间范围选择 → TPS 汇总 → Provider/Model 分组 → 趋势 → 最近样本
 *
 * TPS 语义（与后端 `tps::compute_tps` 对齐）：
 * - **每条样本 = 单次请求**（一次对话 turn），并发请求各自独立，不会叠加
 * - TPS = output_tokens / generation_seconds
 * - generation = 流式且 first_token 可信时用 (duration - first_token)，否则用 duration
 */

type RangePreset = "1h" | "24h" | "7d" | "today" | "all";

/** 与后端 `generation_ms` 对齐：用于表格展示「生成耗时」，使 输出/耗时 可反推 TPS */
function generationMs(sample: TpsSample): number | null {
  const duration = sample.durationMs;
  if (duration == null || duration <= 0) return null;
  if (!sample.isStreaming || sample.firstTokenMs == null) return duration;
  if (sample.firstTokenMs >= duration) return duration;
  const gen = duration - sample.firstTokenMs;
  // 与后端 is_unreliable_generation_window 一致
  if (gen < 20 && duration > 100) return duration;
  if (duration >= 500 && gen * 50 < duration) return duration;
  return gen;
}

function formatBucketLabel(
  bucketSec: number,
  rangePreset: RangePreset,
): string {
  const d = new Date(bucketSec * 1000);
  if (
    rangePreset === "1h" ||
    rangePreset === "24h" ||
    rangePreset === "today"
  ) {
    return d.toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
    });
  }
  return d.toLocaleDateString(undefined, {
    month: "numeric",
    day: "numeric",
    hour: rangePreset === "7d" ? "2-digit" : undefined,
  });
}

const RANGE_PRESETS: {
  value: RangePreset;
  labelKey: string;
  defaultLabel: string;
}[] = [
  { value: "1h", labelKey: "tpsMonitor.range1h", defaultLabel: "近1小时" },
  { value: "24h", labelKey: "tpsMonitor.range24h", defaultLabel: "近24小时" },
  { value: "7d", labelKey: "tpsMonitor.range7d", defaultLabel: "近7天" },
  { value: "today", labelKey: "tpsMonitor.rangeToday", defaultLabel: "今天" },
  { value: "all", labelKey: "tpsMonitor.rangeAll", defaultLabel: "全部" },
];

/** 计算某 preset 的起始 unix 秒（undefined = 不限） */
function rangeStartSeconds(
  preset: RangePreset,
  nowSec: number,
): number | undefined {
  switch (preset) {
    case "1h":
      return nowSec - 3600;
    case "24h":
      return nowSec - 86_400;
    case "7d":
      return nowSec - 7 * 86_400;
    case "today": {
      const d = new Date(nowSec * 1000);
      d.setHours(0, 0, 0, 0);
      return Math.floor(d.getTime() / 1000);
    }
    case "all":
      return undefined;
  }
}

export default function TpsMonitorPanel() {
  const { t } = useTranslation();
  useTpsEventBridge();

  const [rangePreset, setRangePreset] = useState<RangePreset>("24h");
  // 滑动窗口：每分钟更新一次基准 now，让 "近N" 范围自然前移（query key 随之变化）
  const [nowBucket, setNowBucket] = useState(() =>
    Math.floor(Date.now() / 60_000),
  );
  useEffect(() => {
    const id = setInterval(
      () => setNowBucket(Math.floor(Date.now() / 60_000)),
      60_000,
    );
    return () => clearInterval(id);
  }, []);

  const filters = useMemo(() => {
    const start = rangeStartSeconds(rangePreset, nowBucket * 60);
    return start === undefined ? {} : { startDate: start };
  }, [rangePreset, nowBucket]);

  const { data: enabledData } = useTpsEnabled();
  const enabled = enabledData ?? true;
  const setEnabledMutation = useSetTpsEnabled();
  const clearMutation = useClearTpsSamples();

  const { data: summary, isLoading: summaryLoading } = useTpsSummary(filters);
  const { data: trend } = useTpsTrend(filters, 48);
  const { data: recent, isLoading: recentLoading } = useTpsRecent({}, 50);
  const providerBreakdown = useTpsBreakdown(filters, "provider");
  const modelBreakdown = useTpsBreakdown(filters, "model");

  // 并发（独立特性，2 秒轮询）
  const { data: concurrency } = useConcurrency();
  const resetPeakMutation = useResetConcurrencyPeak();

  const maxTrendTps = useMemo(() => {
    if (!trend || trend.length === 0) return 0;
    return Math.max(...trend.map((p) => p.avgTps), 0.0001);
  }, [trend]);

  const maxConcurrency = useMemo(() => {
    if (!concurrency || concurrency.history.length === 0) return 1;
    return Math.max(
      ...concurrency.history.map((s) => s.count),
      concurrency?.current ?? 0,
      1,
    );
  }, [concurrency]);

  const handleToggle = async (next: boolean) => {
    try {
      await setEnabledMutation.mutateAsync(next);
      toast.success(
        next
          ? t("tpsMonitor.toggleToast", { defaultValue: "TPS 记录已开启" })
          : t("tpsMonitor.toggleToastOff", { defaultValue: "TPS 记录已关闭" }),
      );
    } catch {
      toast.error(t("tpsMonitor.toggleError", { defaultValue: "切换失败" }));
    }
  };

  const handleClear = async () => {
    try {
      await clearMutation.mutateAsync(undefined);
      toast.success(
        t("tpsMonitor.clearToast", { defaultValue: "已清空 TPS 记录" }),
      );
    } catch {
      toast.error(t("tpsMonitor.clearError", { defaultValue: "清空失败" }));
    }
  };

  const handleResetPeak = async () => {
    try {
      await resetPeakMutation.mutateAsync();
      toast.success(
        t("tpsMonitor.resetPeakToast", { defaultValue: "峰值已重置" }),
      );
    } catch {
      toast.error(t("tpsMonitor.resetPeakError", { defaultValue: "重置失败" }));
    }
  };

  return (
    <div className="px-6 pt-4 pb-12 flex flex-col gap-4 overflow-y-auto">
      {/* 标题栏：开关 + 清空 */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Gauge className="w-5 h-5 text-primary" />
          <span className="text-sm text-muted-foreground">
            {t("tpsMonitor.subtitle", {
              defaultValue:
                "按单次请求统计输出速度（tok/s）；并发请求各自独立采样，不会叠加",
            })}
          </span>
        </div>
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <span className="text-sm text-muted-foreground">
              {t("tpsMonitor.enable", { defaultValue: "记录" })}
            </span>
            <Switch
              checked={enabled}
              onCheckedChange={handleToggle}
              disabled={setEnabledMutation.isPending}
            />
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={handleClear}
            disabled={clearMutation.isPending}
            className="gap-1.5"
          >
            <Trash2 className="w-4 h-4" />
            {t("tpsMonitor.clear", { defaultValue: "清空" })}
          </Button>
        </div>
      </div>

      {/* 并发监控（实时） */}
      <Card>
        <CardHeader className="pb-2">
          <div className="flex items-center justify-between">
            <CardTitle className="text-sm flex items-center gap-2">
              <Users className="w-4 h-4 text-success" />
              {t("tpsMonitor.concurrencyTitle", { defaultValue: "并发监控" })}
            </CardTitle>
            <Button
              variant="ghost"
              size="sm"
              onClick={handleResetPeak}
              disabled={resetPeakMutation.isPending}
              className="gap-1.5 h-7"
            >
              <RotateCcw className="w-3.5 h-3.5" />
              {t("tpsMonitor.resetPeak", { defaultValue: "重置峰值" })}
            </Button>
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="grid grid-cols-2 gap-3">
            <ConcurrencyStat
              label={t("tpsMonitor.currentConcurrency", {
                defaultValue: "当前并发",
              })}
              value={concurrency?.current ?? 0}
              accent="emerald"
            />
            <ConcurrencyStat
              label={t("tpsMonitor.peakConcurrency", {
                defaultValue: "峰值并发",
              })}
              value={concurrency?.peak ?? 0}
              accent="blue"
            />
          </div>
          {concurrency && concurrency.history.length > 1 ? (
            <div className="flex items-end gap-0.5 h-16">
              {concurrency.history.map((s, i) => {
                const h = Math.max(2, (s.count / maxConcurrency) * 100);
                return (
                  <div
                    key={`${s.ts}-${i}`}
                    className="flex-1 min-w-[2px] bg-emerald-500/60 dark:bg-emerald-400/60 rounded-t-sm"
                    style={{ height: `${h}%` }}
                    title={`${s.count}`}
                  />
                );
              })}
            </div>
          ) : (
            <div className="text-xs text-muted-foreground text-center py-2">
              {t("tpsMonitor.concurrencyHint", {
                defaultValue: "代理运行时实时统计在途请求数",
              })}
            </div>
          )}
        </CardContent>
      </Card>

      {/* 时间范围选择 */}
      <div className="flex items-center gap-2 flex-wrap">
        <span className="text-sm text-muted-foreground">
          {t("tpsMonitor.rangeLabel", { defaultValue: "时间范围" })}
        </span>
        <div className="flex items-center gap-1">
          {RANGE_PRESETS.map((p) => (
            <Button
              key={p.value}
              variant={rangePreset === p.value ? "default" : "outline"}
              size="sm"
              onClick={() => setRangePreset(p.value)}
              className="h-7"
            >
              {t(p.labelKey, { defaultValue: p.defaultLabel })}
            </Button>
          ))}
        </div>
      </div>

      {/* TPS 汇总卡片 */}
      <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-3">
        <StatCard
          icon={<Activity className="w-4 h-4" />}
          label={t("tpsMonitor.avgTps", { defaultValue: "平均 TPS" })}
          value={summary ? summary.avgTps.toFixed(1) : "—"}
          unit="tok/s"
          loading={summaryLoading}
        />
        <StatCard
          icon={<Zap className="w-4 h-4" />}
          label={t("tpsMonitor.maxTps", { defaultValue: "峰值 TPS" })}
          value={summary ? summary.maxTps.toFixed(1) : "—"}
          unit="tok/s"
          loading={summaryLoading}
        />
        <StatCard
          icon={<Gauge className="w-4 h-4" />}
          label={t("tpsMonitor.p95Tps", { defaultValue: "P95 TPS" })}
          value={summary ? summary.p95Tps.toFixed(1) : "—"}
          unit="tok/s"
          loading={summaryLoading}
        />
        <StatCard
          label={t("tpsMonitor.totalOutput", { defaultValue: "输出 Tokens" })}
          value={summary ? summary.totalOutputTokens.toLocaleString() : "—"}
          loading={summaryLoading}
        />
        <StatCard
          label={t("tpsMonitor.sampleCount", { defaultValue: "样本数" })}
          value={summary ? summary.sampleCount.toLocaleString() : "—"}
          loading={summaryLoading}
        />
      </div>

      {/* Provider / Model 分组 */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <BreakdownCard
          title={t("tpsMonitor.providerBreakdown", {
            defaultValue: "按 Provider 分组",
          })}
          rows={providerBreakdown.data}
          loading={providerBreakdown.isLoading}
          showApp
          emptyText={t("tpsMonitor.noBreakdown", {
            defaultValue: "该范围内暂无样本",
          })}
          t={t}
        />
        <BreakdownCard
          title={t("tpsMonitor.modelBreakdown", {
            defaultValue: "按 Model 分组",
          })}
          rows={modelBreakdown.data}
          loading={modelBreakdown.isLoading}
          showApp={false}
          emptyText={t("tpsMonitor.noBreakdown", {
            defaultValue: "该范围内暂无样本",
          })}
          t={t}
        />
      </div>

      {/* 趋势条形图 */}
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">
            {t("tpsMonitor.trend", { defaultValue: "TPS 趋势" })}
          </CardTitle>
          <p className="text-xs text-muted-foreground mt-1">
            {t("tpsMonitor.trendHint", {
              defaultValue:
                "每个柱 = 该时间桶内所有请求的平均 TPS（单请求 tok/s 的均值，非并发吞吐叠加）。柱高相对峰值归一化。",
            })}
          </p>
        </CardHeader>
        <CardContent>
          {trend && trend.length > 0 ? (
            <TrendChart
              points={trend}
              maxTps={maxTrendTps}
              rangePreset={rangePreset}
              t={t}
            />
          ) : (
            <div className="text-sm text-muted-foreground py-8 text-center">
              {t("tpsMonitor.emptyTrend", { defaultValue: "暂无趋势数据" })}
            </div>
          )}
        </CardContent>
      </Card>

      {/* 最近样本 */}
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">
            {t("tpsMonitor.recent", { defaultValue: "最近样本" })}
          </CardTitle>
          <p className="text-xs text-muted-foreground mt-1">
            {t("tpsMonitor.recentHint", {
              defaultValue:
                "TPS = 输出 Tokens ÷ 生成耗时(秒)。生成耗时优先用 (总耗时 − 首 token)；首 token 不可信时回退总耗时。每行 = 单次请求。",
            })}
          </p>
        </CardHeader>
        <CardContent>
          {recent && recent.length > 0 ? (
            <div className="max-h-80 overflow-y-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>
                      {t("tpsMonitor.colTime", { defaultValue: "时间" })}
                    </TableHead>
                    <TableHead>
                      {t("tpsMonitor.colApp", { defaultValue: "应用" })}
                    </TableHead>
                    <TableHead>
                      {t("tpsMonitor.colModel", { defaultValue: "模型" })}
                    </TableHead>
                    <TableHead className="text-right">
                      {t("tpsMonitor.colOutput", { defaultValue: "输出" })}
                    </TableHead>
                    <TableHead className="text-right">
                      {t("tpsMonitor.colDuration", {
                        defaultValue: "总耗时",
                      })}
                    </TableHead>
                    <TableHead className="text-right">
                      {t("tpsMonitor.colGenDuration", {
                        defaultValue: "生成耗时",
                      })}
                    </TableHead>
                    <TableHead className="text-right">
                      {t("tpsMonitor.colTps", { defaultValue: "TPS" })}
                    </TableHead>
                    <TableHead>
                      {t("tpsMonitor.colStream", { defaultValue: "流式" })}
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {recent.map((s) => {
                    const gen = generationMs(s);
                    // 用 输出/生成耗时 现场重算，保证表格可反推；同时修正历史
                    // 错误 first_token 落库导致的离群 tps（汇总卡片仍读库内值，
                    // 建议清空后重新采样）。
                    const displayTps =
                      gen != null && gen > 0 && s.outputTokens > 0
                        ? (s.outputTokens * 1000) / gen
                        : s.tps;
                    return (
                      <TableRow key={s.id}>
                        <TableCell className="tnum text-xs whitespace-nowrap">
                          {new Date(s.createdAt * 1000).toLocaleTimeString()}
                        </TableCell>
                        <TableCell className="text-xs">{s.appType}</TableCell>
                        <TableCell className="font-mono text-xs">
                          {s.model}
                        </TableCell>
                        <TableCell className="text-right tnum text-xs">
                          {s.outputTokens.toLocaleString()}
                        </TableCell>
                        <TableCell className="text-right tnum text-xs text-muted-foreground">
                          {s.durationMs != null
                            ? `${s.durationMs.toLocaleString()} ms`
                            : "—"}
                        </TableCell>
                        <TableCell
                          className="text-right tnum text-xs"
                          title={
                            s.firstTokenMs != null
                              ? `first_token=${s.firstTokenMs}ms`
                              : undefined
                          }
                        >
                          {gen != null ? `${gen.toLocaleString()} ms` : "—"}
                        </TableCell>
                        <TableCell className="text-right tnum text-xs">
                          <span
                            className={cn(
                              displayTps >= 50
                                ? "text-success"
                                : displayTps >= 10
                                  ? "text-primary"
                                  : "text-muted-foreground",
                            )}
                          >
                            {displayTps.toFixed(1)}
                          </span>
                        </TableCell>
                        <TableCell>
                          {s.isStreaming ? (
                            <Badge variant="secondary">stream</Badge>
                          ) : (
                            <Badge variant="outline">sync</Badge>
                          )}
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            </div>
          ) : recentLoading ? (
            <div className="text-sm text-muted-foreground py-8 text-center">
              {t("tpsMonitor.loading", { defaultValue: "加载中…" })}
            </div>
          ) : (
            <div className="text-sm text-muted-foreground py-8 text-center">
              {enabled
                ? t("tpsMonitor.empty", {
                    defaultValue: "暂无样本，发起一次请求后会自动记录",
                  })
                : t("tpsMonitor.emptyDisabled", {
                    defaultValue: "TPS 记录已关闭",
                  })}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

// ── 子组件 ──────────────────────────────────────────────────────────────────

interface TrendChartProps {
  points: TpsTrendPoint[];
  maxTps: number;
  rangePreset: RangePreset;
  t: (key: string, opts?: Record<string, unknown>) => string;
}

function TrendChart({ points, maxTps, rangePreset, t }: TrendChartProps) {
  const first = points[0];
  const last = points[points.length - 1];
  const mid = points[Math.floor(points.length / 2)];
  const yTicks = [maxTps, maxTps / 2, 0];

  return (
    <div className="flex flex-col gap-1">
      <div className="flex gap-2">
        {/* Y 轴刻度 */}
        <div className="flex flex-col justify-between w-12 shrink-0 h-32 text-[11px] text-muted-foreground tnum text-right pr-1">
          {yTicks.map((v, i) => (
            <span key={i}>{v.toFixed(0)}</span>
          ))}
        </div>
        {/* 柱状区域 */}
        <div className="flex-1 flex flex-col min-w-0">
          <div className="flex items-end gap-0.5 h-32 border-l border-b border-border/60 pl-0.5">
            {points.map((p) => {
              const h = Math.max(2, (p.avgTps / maxTps) * 100);
              const label = formatBucketLabel(p.bucket, rangePreset);
              return (
                <div
                  key={p.bucket}
                  className="flex-1 min-w-[2px] bg-primary/70 rounded-t-sm transition-colors duration-150 hover:bg-primary"
                  style={{ height: `${h}%` }}
                  title={`${label}\n${t("tpsMonitor.trendBarTooltip", {
                    defaultValue:
                      "平均 {{tps}} tok/s · {{count}} 个请求 · 输出 {{tokens}} tokens",
                    tps: p.avgTps.toFixed(1),
                    count: p.sampleCount,
                    tokens: p.totalOutputTokens.toLocaleString(),
                  })}`}
                />
              );
            })}
          </div>
          {/* X 轴时间标签 */}
          <div className="flex justify-between text-[11px] text-muted-foreground tnum mt-1 pl-0.5">
            <span>{formatBucketLabel(first.bucket, rangePreset)}</span>
            {points.length > 2 && mid && (
              <span className="opacity-70">
                {formatBucketLabel(mid.bucket, rangePreset)}
              </span>
            )}
            <span>{formatBucketLabel(last.bucket, rangePreset)}</span>
          </div>
        </div>
      </div>
      <div className="flex items-center justify-between text-[11px] text-muted-foreground pl-14">
        <span>
          {t("tpsMonitor.trendYAxis", {
            defaultValue: "纵轴：平均 TPS (tok/s)",
          })}
        </span>
        <span>
          {t("tpsMonitor.trendXAxis", { defaultValue: "横轴：时间" })}
        </span>
      </div>
    </div>
  );
}

interface StatCardProps {
  icon?: React.ReactNode;
  label: string;
  value: string;
  unit?: string;
  loading?: boolean;
}

function StatCard({ icon, label, value, unit, loading }: StatCardProps) {
  return (
    <Card>
      <CardContent className="pt-4 pb-4">
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground mb-1">
          {icon}
          {label}
        </div>
        <div className="flex items-baseline gap-1">
          <span className="text-xl font-semibold tnum">
            {loading ? "—" : value}
          </span>
          {unit && (
            <span className="text-xs text-muted-foreground">{unit}</span>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

interface ConcurrencyStatProps {
  label: string;
  value: number;
  accent: "emerald" | "blue";
}

function ConcurrencyStat({ label, value, accent }: ConcurrencyStatProps) {
  return (
    <div className="rounded-xl border border-border/60 bg-muted/40 px-3 py-2">
      <div className="text-xs text-muted-foreground mb-0.5">{label}</div>
      <div
        className={cn(
          "text-2xl font-semibold tnum",
          accent === "emerald" ? "text-success" : "text-primary",
        )}
      >
        {value}
      </div>
    </div>
  );
}

interface BreakdownCardProps {
  title: string;
  rows: TpsGroupStats[] | undefined;
  loading: boolean;
  showApp: boolean;
  emptyText: string;
  t: (key: string, opts?: Record<string, unknown>) => string;
}

function BreakdownCard({
  title,
  rows,
  loading,
  showApp,
  emptyText,
  t,
}: BreakdownCardProps) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">{title}</CardTitle>
      </CardHeader>
      <CardContent>
        {rows && rows.length > 0 ? (
          <div className="max-h-72 overflow-y-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>
                    {t("tpsMonitor.colDisplayName", { defaultValue: "名称" })}
                  </TableHead>
                  {showApp && (
                    <TableHead>
                      {t("tpsMonitor.colApp", { defaultValue: "应用" })}
                    </TableHead>
                  )}
                  <TableHead className="text-right">
                    {t("tpsMonitor.colSampleCount", { defaultValue: "样本数" })}
                  </TableHead>
                  <TableHead className="text-right">
                    {t("tpsMonitor.colAvgTps", { defaultValue: "平均" })}
                  </TableHead>
                  <TableHead className="text-right">
                    {t("tpsMonitor.colMaxTps", { defaultValue: "峰值" })}
                  </TableHead>
                  <TableHead className="text-right">P95</TableHead>
                  <TableHead className="text-right">
                    {t("tpsMonitor.colTotalOutput", {
                      defaultValue: "输出Tokens",
                    })}
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.map((r) => (
                  <TableRow key={`${r.appType ?? ""}:${r.key}`}>
                    <TableCell
                      className="font-mono text-xs max-w-[180px] truncate"
                      title={r.key}
                    >
                      {r.displayName ?? r.key}
                    </TableCell>
                    {showApp && (
                      <TableCell className="text-xs">
                        {r.appType ?? "—"}
                      </TableCell>
                    )}
                    <TableCell className="text-right tnum text-xs">
                      {r.sampleCount.toLocaleString()}
                    </TableCell>
                    <TableCell className="text-right tnum text-xs text-primary">
                      {r.avgTps.toFixed(1)}
                    </TableCell>
                    <TableCell className="text-right tnum text-xs text-success">
                      {r.maxTps.toFixed(1)}
                    </TableCell>
                    <TableCell className="text-right tnum text-xs">
                      {r.p95Tps.toFixed(1)}
                    </TableCell>
                    <TableCell className="text-right tnum text-xs">
                      {r.totalOutputTokens.toLocaleString()}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        ) : loading ? (
          <div className="text-sm text-muted-foreground py-6 text-center">
            {t("tpsMonitor.loading", { defaultValue: "加载中…" })}
          </div>
        ) : (
          <div className="text-sm text-muted-foreground py-6 text-center">
            {emptyText}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
