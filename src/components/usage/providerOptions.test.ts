import { describe, expect, it } from "vitest";
import { aggregateProviderOptions } from "@/components/usage/providerOptions";
import type { ProviderStats } from "@/types/usage";

/** 造一行 provider 统计：只关心聚合用到的三个字段，其余填占位值。 */
function stat(
  name: string,
  isDeleted: boolean,
  overrides: Partial<ProviderStats> = {},
): ProviderStats {
  return {
    providerId: `id-${name}-${isDeleted ? "del" : "live"}-${Math.random()}`,
    providerName: name,
    providerIsDeleted: isDeleted,
    requestCount: 1,
    totalTokens: 1,
    totalCost: "0.000000",
    successRate: 100,
    avgLatencyMs: 0,
    ...overrides,
  };
}

describe("aggregateProviderOptions", () => {
  it("returns an empty list when there is no data and nothing selected", () => {
    expect(aggregateProviderOptions(undefined, undefined, false)).toEqual([]);
    expect(aggregateProviderOptions([], undefined, false)).toEqual([]);
  });

  it("marks a deleted-only provider as deleted", () => {
    const options = aggregateProviderOptions(
      [stat("MyAPI", true), stat("MyAPI", true)],
      undefined,
      false,
    );
    expect(options).toEqual([{ name: "MyAPI", isDeleted: true }]);
  });

  it("marks a live provider as not deleted", () => {
    const options = aggregateProviderOptions(
      [stat("Live", false)],
      undefined,
      false,
    );
    expect(options).toEqual([{ name: "Live", isDeleted: false }]);
  });

  // 回归：Gap A —— OR 聚合会给 live 同名 provider 误打「已删除」徽标。
  it("does NOT badge a live provider that shares a name with a deleted one (AND aggregation)", () => {
    const options = aggregateProviderOptions(
      [stat("MyAPI", true), stat("MyAPI", false)],
      undefined,
      false,
    );
    // 同名两行 → 一项；只要有一个 live 同名，就不标已删除。
    expect(options).toEqual([{ name: "MyAPI", isDeleted: false }]);
  });

  it("keeps separate names distinct and dedupes by name", () => {
    const options = aggregateProviderOptions(
      [stat("A", false), stat("B", true), stat("A", false)],
      undefined,
      false,
    );
    expect(options).toEqual([
      { name: "A", isDeleted: false },
      { name: "B", isDeleted: true },
    ]);
  });

  // 回归：Gap B —— 选中项掉出范围时兜底不得把删除态硬编码成 false。
  it("re-adds a missing selected provider using the recorded deleted state", () => {
    const options = aggregateProviderOptions(
      [stat("Other", false)],
      "MyAPI",
      true,
    );
    expect(options).toContainEqual({ name: "MyAPI", isDeleted: true });
  });

  it("re-adds a missing selected live provider as not deleted", () => {
    const options = aggregateProviderOptions(
      [stat("Other", false)],
      "Live",
      false,
    );
    expect(options).toContainEqual({ name: "Live", isDeleted: false });
  });

  it("prefers fresh data over the stale recorded deleted state", () => {
    // 选中项「掉出又回来」或数据更新后：以最新的 providerIsDeleted 为准，
    // 不沿用旧的 selectedIsDeleted。
    const options = aggregateProviderOptions(
      [stat("MyAPI", false)],
      "MyAPI",
      true, // stale recorded state says deleted
    );
    expect(options).toEqual([{ name: "MyAPI", isDeleted: false }]);
  });
});
