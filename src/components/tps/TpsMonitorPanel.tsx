import React, { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Activity, Gauge, Trash2, Zap } from "lucide-react";
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
  useTpsEnabled,
  useSetTpsEnabled,
  useClearTpsSamples,
} from "@/lib/query/tps";
import { useTpsEventBridge } from "@/hooks/useTpsEventBridge";
import { cn } from "@/lib/utils";

/**
 * TPS 监控 & 记录 面板（本地定制，fork 专用）
 *
 * 自包含：所有数据通过 `@/lib/query/tps` 的 hook 拉取，事件通过
 * `useTpsEventBridge` 实时刷新。移除本组件只需删掉 App.tsx 里的 view 分支与
 * nav 按钮，不影响其它功能。
 */
export default function TpsMonitorPanel() {
  const { t } = useTranslation();
  useTpsEventBridge();

  const { data: enabledData } = useTpsEnabled();
  const enabled = enabledData ?? true;
  const setEnabledMutation = useSetTpsEnabled();
  const clearMutation = useClearTpsSamples();

  const { data: summary, isLoading: summaryLoading } = useTpsSummary({});
  const { data: trend } = useTpsTrend({}, 48);
  const { data: recent, isLoading: recentLoading } = useTpsRecent({}, 50);

  const maxTrendTps = useMemo(() => {
    if (!trend || trend.length === 0) return 0;
    return Math.max(...trend.map((p) => p.avgTps), 0.0001);
  }, [trend]);

  const handleToggle = async (next: boolean) => {
    try {
      await setEnabledMutation.mutateAsync(next);
      toast.success(
        next
          ? t("tpsMonitor.toggleToast", { defaultValue: "TPS 记录已开启" })
          : t("tpsMonitor.toggleToastOff", { defaultValue: "TPS 记录已关闭" }),
      );
    } catch {
      // mutation 失败时 hook 不弹 toast，这里兜底
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

  return (
    <div className="px-6 pt-4 pb-12 flex flex-col gap-4 overflow-y-auto">
      {/* 标题栏：开关 + 清空 */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Gauge className="w-5 h-5 text-blue-500" />
          <span className="text-sm text-muted-foreground">
            {t("tpsMonitor.subtitle", {
              defaultValue: "实时统计本地代理每秒输出 token 数",
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

      {/* 汇总卡片 */}
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

      {/* 趋势条形图 */}
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">
            {t("tpsMonitor.trend", { defaultValue: "TPS 趋势" })}
          </CardTitle>
        </CardHeader>
        <CardContent>
          {trend && trend.length > 0 ? (
            <div className="flex items-end gap-0.5 h-32">
              {trend.map((p) => {
                const h = Math.max(2, (p.avgTps / maxTrendTps) * 100);
                return (
                  <div
                    key={p.bucket}
                    className="flex-1 min-w-[2px] bg-blue-500/70 dark:bg-blue-400/70 rounded-t-sm transition-all"
                    style={{ height: `${h}%` }}
                    title={`${p.avgTps.toFixed(1)} tok/s · ${p.sampleCount} 样本`}
                  />
                );
              })}
            </div>
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
                        defaultValue: "耗时(ms)",
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
                  {recent.map((s) => (
                    <TableRow key={s.id}>
                      <TableCell className="font-mono text-xs whitespace-nowrap">
                        {new Date(s.createdAt * 1000).toLocaleTimeString()}
                      </TableCell>
                      <TableCell className="text-xs">{s.appType}</TableCell>
                      <TableCell className="font-mono text-xs">
                        {s.model}
                      </TableCell>
                      <TableCell className="text-right font-mono text-xs">
                        {s.outputTokens.toLocaleString()}
                      </TableCell>
                      <TableCell className="text-right font-mono text-xs">
                        {s.durationMs ?? "—"}
                      </TableCell>
                      <TableCell className="text-right font-mono text-xs">
                        <span
                          className={cn(
                            s.tps >= 50
                              ? "text-emerald-500"
                              : s.tps >= 10
                                ? "text-blue-500"
                                : "text-muted-foreground",
                          )}
                        >
                          {s.tps.toFixed(1)}
                        </span>
                      </TableCell>
                      <TableCell>
                        {s.isStreaming ? (
                          <Badge variant="secondary" className="text-[10px]">
                            stream
                          </Badge>
                        ) : (
                          <Badge variant="outline" className="text-[10px]">
                            sync
                          </Badge>
                        )}
                      </TableCell>
                    </TableRow>
                  ))}
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
          <span className="text-xl font-semibold font-mono">
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
