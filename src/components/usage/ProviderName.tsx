import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

interface ProviderNameProps {
  /** 展示名（已解析为可读名；后端保证不再回退成裸 UUID） */
  name: string;
  /** 该 provider 是否已删除 —— 后端 is_deleted 信号 */
  isDeleted?: boolean | null;
  /** 悬浮 title（缺省用 name；调用方常传 provider_id 以便核对） */
  title?: string;
  className?: string;
}

/**
 * Provider 展示名（带「已删除」视觉）。
 *
 * 删除的 provider 仍会在历史用量 / TPS 分组里出现。为让用户一眼区分
 * 「仍在用的 provider」与「已删除、只剩历史数据的 provider」，已删除者
 * 渲染为：灰化 + 删除线的名字，后跟一个「已删除」中性徽标。
 */
export function ProviderName({ name, isDeleted, title, className }: ProviderNameProps) {
  const { t } = useTranslation();

  if (!isDeleted) {
    return (
      <span className={className} title={title ?? name}>
        {name}
      </span>
    );
  }

  return (
    <span className={cn("inline-flex items-center gap-1.5", className)} title={title ?? name}>
      <span className="text-muted-foreground line-through decoration-muted-foreground/60">
        {name}
      </span>
      <Badge variant="neutral" className="px-1.5 py-0 text-[10px] leading-4">
        {t("common.deleted", "已删除")}
      </Badge>
    </span>
  );
}
