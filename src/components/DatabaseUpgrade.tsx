import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { exit } from "@tauri-apps/plugin-process";
import { AlertTriangle, ExternalLink, FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";

const FORK_RELEASES_URL = "https://github.com/lxinfei5/cc-switch/releases";

interface DatabaseUpgradeProps {
  payload: {
    path?: string;
    error?: string;
    kind?: string;
    db_version?: number;
    supported_version?: number;
  };
}

/**
 * 数据库版本过新时的离线恢复界面。
 *
 * 本 fork 不从官方仓库检查或安装应用更新，因此这里只提示用户手动安装
 * 与本 fork 匹配的构建，并提供配置目录和 fork 发布页入口。
 */
export function DatabaseUpgrade({ payload }: DatabaseUpgradeProps) {
  const { t } = useTranslation();
  const dbVersion = payload.db_version;
  const supportedVersion = payload.supported_version;

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-6 text-foreground">
      <div className="w-full max-w-lg space-y-5 rounded-2xl border border-border/60 bg-card/80 p-7 shadow-xl">
        <div className="flex items-start gap-4">
          <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-xl bg-red-100 text-red-600 dark:bg-red-950/50 dark:text-red-400">
            <AlertTriangle className="h-6 w-6" />
          </div>
          <div className="space-y-1">
            <h1 className="text-lg font-semibold">
              {t("dbUpgrade.title", "数据库版本过新")}
            </h1>
            <p className="text-sm text-muted-foreground">
              {t(
                "dbUpgrade.description",
                "当前数据库由更高版本的 CC Switch 创建。请手动安装与你的 fork 匹配的构建；本构建不会自动从官方仓库下载更新。",
              )}
            </p>
            {dbVersion != null && supportedVersion != null && (
              <p className="pt-0.5 text-xs text-muted-foreground tabular-nums">
                {t("dbUpgrade.versionInfo", {
                  db: dbVersion,
                  supported: supportedVersion,
                })}
              </p>
            )}
          </div>
        </div>

        <div className="space-y-1 rounded-lg border border-border/50 bg-muted/40 p-3 text-xs text-muted-foreground">
          {payload.error && (
            <p className="break-words font-mono">{payload.error}</p>
          )}
          {payload.path && (
            <p className="break-all">
              {t("dbUpgrade.dbPath", "数据库文件")}：{payload.path}
            </p>
          )}
        </div>

        <div className="space-y-2 rounded-lg border border-red-300/60 bg-red-50 p-3 text-sm text-red-700 dark:border-red-500/40 dark:bg-red-950/40 dark:text-red-300">
          <p className="font-medium">
            {t("dbUpgrade.incompatibleTitle", "需要手动安装匹配的构建")}
          </p>
          <p className="leading-relaxed">
            {t("dbUpgrade.incompatibleDescription", {
              db: dbVersion,
              supported: supportedVersion,
              defaultValue:
                "数据库版本（v{{db}}）高于本构建支持的版本（v{{supported}}）。请从自己的 fork 编译并安装支持该数据库版本的构建；本应用不会自动从官方仓库检查或下载更新。",
            })}
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            className="gap-2"
            onClick={() =>
              void invoke("open_external", { url: FORK_RELEASES_URL })
            }
          >
            <ExternalLink className="h-4 w-4" />
            {t("dbUpgrade.openReleases", "打开 fork 发布页")}
          </Button>

          <Button
            variant="outline"
            className="gap-2"
            onClick={() => void invoke("open_app_config_folder")}
          >
            <FolderOpen className="h-4 w-4" />
            {t("dbUpgrade.openConfigDir", "打开配置目录")}
          </Button>

          <Button
            variant="ghost"
            className="ml-auto text-muted-foreground"
            onClick={() => void exit(0)}
          >
            {t("dbUpgrade.quit", "退出")}
          </Button>
        </div>
      </div>
    </div>
  );
}

export default DatabaseUpgrade;
