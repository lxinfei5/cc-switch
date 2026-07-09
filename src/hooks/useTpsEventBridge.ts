import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { tpsKeys } from "@/lib/query/tps";

/**
 * 监听后端 `tps-recorded` 事件，收到后立刻 invalidate 所有 TPS 相关查询，
 * 让 TPS 监控面板实时刷新（无需轮询）。
 *
 * 后端在 `tps_samples` 写入新行时会 emit 该事件（200ms 防抖合并）。
 * 仅在 TpsMonitorPanel 挂载时生效，离开页面自动取消监听。
 *
 * 本地定制（fork 专用）：与 useUsageEventBridge 解耦，互不影响。
 */
export function useTpsEventBridge() {
  const queryClient = useQueryClient();

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let disposed = false;

    (async () => {
      const off = await listen("tps-recorded", () => {
        queryClient.invalidateQueries({ queryKey: tpsKeys.all });
      });

      if (disposed) {
        off();
      } else {
        unlisten = off;
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [queryClient]);
}
