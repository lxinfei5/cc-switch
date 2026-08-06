import type { ProviderStats } from "@/types/usage";

/** 来源筛选下拉的一个选项（按展示名去重后的 Provider）。 */
export interface ProviderOption {
  name: string;
  isDeleted: boolean;
}

/**
 * 把 Provider 统计行聚合为来源筛选下拉的选项列表。
 *
 * 按展示名去重，并正确携带「该名字下所有 provider 是否已删除」：
 *
 * - **AND 聚合（不是 OR）**：`get_provider_stats` 按 `(provider_id, app_type)`
 *   分组，所以「删除旧 MyAPI 后又新建同名 live MyAPI」会返回两行、同名
 *   `providerName` 但 `providerIsDeleted` 一真一假。这里只有**全部**同名行
 *   都已删除时才标 `isDeleted=true`——只要还存在一个 live 同名 provider 就不
 *   打「已删除」徽标，避免给在用的 provider 误标删除。
 *
 * - **选中项兜底**：数据刷新后选中项可能掉出当前范围（如改了时间范围、该
 *   provider 在新范围无数据），为让 Select 仍能渲染选中文案、用户看得见才能
 *   主动清除，会把选中名补回列表。此时它的删除态沿用 `selectedIsDeleted`
 *   （选中那一刻从当时选项里捕获），而非硬编码 `false`，避免误丢「已删除」徽标。
 */
export function aggregateProviderOptions(
  stats: ProviderStats[] | undefined,
  selectedName: string | undefined,
  selectedIsDeleted: boolean,
): ProviderOption[] {
  const byName = new Map<string, boolean>();

  for (const stat of stats ?? []) {
    // AND：缺省 true，仅当“之前的行与当前行都已删除”才保持 true。
    byName.set(
      stat.providerName,
      (byName.get(stat.providerName) ?? true) &&
        Boolean(stat.providerIsDeleted),
    );
  }

  // 选中项掉出列表时补回，删除态沿用上次的记录。
  if (selectedName && !byName.has(selectedName)) {
    byName.set(selectedName, selectedIsDeleted);
  }

  return Array.from(byName, ([name, isDeleted]) => ({ name, isDeleted }));
}
