import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageDashboard } from "@/components/usage/UsageDashboard";
import type { ProviderStats } from "@/types/usage";

const useProviderStatsMock = vi.hoisted(() => vi.fn());
const useModelStatsMock = vi.hoisted(() => vi.fn());
// 收集每个 Select 的 onValueChange，按挂载顺序与 DOM 顺序一致，供 SelectItem 点击时回调。
const selectChangeHandlers = vi.hoisted(() => [] as Array<(v: string) => void>);

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string) => fallback ?? key,
    i18n: {
      resolvedLanguage: "en",
      language: "en",
    },
  }),
}));

vi.mock("framer-motion", () => ({
  motion: {
    div: ({ children, ...props }: any) => <div {...props}>{children}</div>,
  },
}));

vi.mock("@/hooks/useUsageEventBridge", () => ({
  useUsageEventBridge: () => {},
}));

vi.mock("@/lib/query/usage", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/query/usage")>(
      "@/lib/query/usage",
    );
  return {
    ...actual,
    useProviderStats: (...args: unknown[]) => useProviderStatsMock(...args),
    useModelStats: (...args: unknown[]) => useModelStatsMock(...args),
  };
});

vi.mock("@/components/usage/UsageHero", () => ({
  UsageHero: () => <div data-testid="usage-hero" />,
}));

vi.mock("@/components/usage/UsageTrendChart", () => ({
  UsageTrendChart: () => <div data-testid="usage-trend" />,
}));

vi.mock("@/components/usage/RequestLogTable", () => ({
  RequestLogTable: () => <div data-testid="request-log-table" />,
}));

vi.mock("@/components/usage/ProviderStatsTable", () => ({
  ProviderStatsTable: () => <div data-testid="provider-stats-table" />,
}));

vi.mock("@/components/usage/ModelStatsTable", () => ({
  ModelStatsTable: () => <div data-testid="model-stats-table" />,
}));

vi.mock("@/components/usage/PricingConfigPanel", () => ({
  PricingConfigPanel: () => <div data-testid="pricing-config-panel" />,
}));

vi.mock("@/components/usage/UsageDateRangePicker", () => ({
  UsageDateRangePicker: () => <button type="button">date-range</button>,
}));

vi.mock("@/components/ui/select", () => {
  // 按创建顺序登记每个 Select 的 onValueChange；SelectItem 通过外层包一层
  // [data-select-idx] 找到自己所属的 Select 索引，点击时回调对应的 handler。
  let selectCount = 0;
  return {
    Select: ({ value, onValueChange, children }: any) => {
      const idx = selectCount++;
      selectChangeHandlers[idx] = onValueChange;
      return (
        <div data-testid={`select-${value}`} data-select-idx={idx}>
          {children}
          <button type="button" onClick={() => onValueChange?.("5000")}>
            choose-5000
          </button>
        </div>
      );
    },
    SelectTrigger: ({ children, ...props }: any) => (
      <button type="button" {...props}>
        {children}
      </button>
    ),
    SelectValue: () => null,
    SelectContent: ({ children }: any) => <div>{children}</div>,
    SelectItem: ({ children, value, ...props }: any) => (
      <div
        role="option"
        aria-selected="false"
        onClick={(e) => {
          const host = (e.currentTarget as HTMLElement).closest(
            "[data-select-idx]",
          ) as HTMLElement | null;
          const idx = host ? Number(host.dataset.selectIdx) : -1;
          selectChangeHandlers[idx]?.(value);
        }}
        {...props}
      >
        {children}
      </div>
    ),
  };
});

const renderDashboard = (props: ComponentProps<typeof UsageDashboard> = {}) => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <UsageDashboard {...props} />
    </QueryClientProvider>,
  );
};

/** 造一行 provider 统计：聚合只用到 providerName / providerIsDeleted，其余填占位值。 */
const providerStat = (
  name: string,
  isDeleted: boolean,
  suffix: string,
): ProviderStats => ({
  providerId: `id-${name}-${suffix}`,
  providerName: name,
  providerIsDeleted: isDeleted,
  requestCount: 1,
  totalTokens: 1,
  totalCost: "0.000000",
  successRate: 100,
  avgLatencyMs: 0,
});

describe("UsageDashboard", () => {
  beforeEach(() => {
    useProviderStatsMock.mockReset();
    useModelStatsMock.mockReset();
    useProviderStatsMock.mockReturnValue({ data: [] });
    useModelStatsMock.mockReturnValue({ data: [] });
  });

  it("uses the saved refresh interval when mounted", () => {
    renderDashboard({ refreshIntervalMs: 5000 });

    expect(screen.getByTestId("select-5000")).toBeInTheDocument();
  });

  it("persists refresh interval changes", async () => {
    const onRefreshIntervalChange = vi.fn().mockResolvedValue(true);
    renderDashboard({ onRefreshIntervalChange });

    fireEvent.click(
      within(screen.getByTestId("select-30000")).getByRole("button", {
        name: "choose-5000",
      }),
    );

    await waitFor(() =>
      expect(onRefreshIntervalChange).toHaveBeenCalledWith(5000),
    );
    expect(screen.getByTestId("select-5000")).toBeInTheDocument();
  });

  it("rolls back optimistic interval changes when persistence fails", async () => {
    const onRefreshIntervalChange = vi.fn().mockResolvedValue(false);
    renderDashboard({ onRefreshIntervalChange });

    fireEvent.click(
      within(screen.getByTestId("select-30000")).getByRole("button", {
        name: "choose-5000",
      }),
    );

    await waitFor(() =>
      expect(onRefreshIntervalChange).toHaveBeenCalledWith(5000),
    );
    await waitFor(() =>
      expect(screen.getByTestId("select-30000")).toBeInTheDocument(),
    );
  });

  // 回归：Gap B —— 选中已删除 provider 后，即使它掉出当前时间范围（无数据行），
  // 下拉项仍应保留「已删除」徽标，而不是被兜底逻辑硬编码成 false。
  it("keeps the deleted badge on a selected provider that drops out of range", async () => {
    useProviderStatsMock.mockReturnValue({
      data: [providerStat("MyAPI", true, "a")],
    });
    const { rerender } = renderDashboard();

    const option = getProviderOption("MyAPI");
    fireEvent.click(option); // 选中 MyAPI（此刻捕获其删除态）
    await waitFor(() =>
      expect(screen.getByTestId("select-v:MyAPI")).toBeInTheDocument(),
    );

    // 时间范围变化 → MyAPI 在新范围无数据；补回下拉时必须沿用捕获的删除态。
    useProviderStatsMock.mockReturnValue({ data: [] });
    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <UsageDashboard />
      </QueryClientProvider>,
    );
    const ghost = getProviderOption("MyAPI");
    expect(within(ghost).getByText("已删除")).toBeInTheDocument();
  });

  // 回归：Gap A —— live provider 与已删除 provider 同名时，不应误打「已删除」徽标。
  it("does not badge a live provider that shares a name with a deleted one", async () => {
    useProviderStatsMock.mockReturnValue({
      data: [
        providerStat("MyAPI", true, "old"),
        providerStat("MyAPI", false, "new"),
      ],
    });
    renderDashboard();

    const option = getProviderOption("MyAPI");
    expect(within(option).queryByText("已删除")).not.toBeInTheDocument();
  });
});

/** 取来源筛选下拉里 title 为 name 的「选项」元素（role=option，排除触发按钮）。 */
function getProviderOption(name: string): HTMLElement {
  const item = screen
    .getAllByRole("option")
    .find((el) => el.getAttribute("title") === name);
  if (!item) {
    throw new Error(`no SelectItem option with title "${name}"`);
  }
  return item;
}
