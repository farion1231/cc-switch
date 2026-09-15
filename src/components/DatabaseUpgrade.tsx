import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { exit } from "@tauri-apps/plugin-process";
import {
  Database,
  Download,
  RefreshCw,
  ExternalLink,
  FolderOpen,
  Loader2,
  AlertTriangle,
} from "lucide-react";
import { Button } from "@/components/ui/button";

const RELEASES_URL = "https://github.com/farion1231/cc-switch/releases";

interface DatabaseUpgradeProps {
  payload: {
    path?: string;
    error?: string;
    kind?: string;
    db_version?: number;
    supported_version?: number;
  };
}

// checking: 启动时检查是否有可用更新
// upgradable: 有可用更新，升级应用即可解决
// incompatible: 已是最新版本但数据库仍过新（可能来自第三方客户端），升级无法解决
// updating: 正在下载/安装更新
// error: 升级过程出错
type Phase = "checking" | "upgradable" | "incompatible" | "updating" | "error";

interface DownloadProgress {
  downloaded: number;
  total: number | null;
}

/**
 * 数据库版本过新（应用过旧）时的应用内恢复界面。
 *
 * 启动时先检查是否有可用更新：
 * - 有 → 提供「升级应用」一键下载+安装+重启，并展示下载进度条。
 * - 无 → 说明当前已是最新版本但数据库仍不兼容（通常由第三方客户端或更高版本创建），
 *   升级无法解决，及时提醒用户备份后改用兼容客户端或等待官方支持。
 */
export function DatabaseUpgrade({ payload }: DatabaseUpgradeProps) {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<Phase>("checking");
  const [availableVersion, setAvailableVersion] = useState<string | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);

  const dbVersion = payload.db_version;
  const supportedVersion = payload.supported_version;

  // 启动时检查可用更新，决定 upgradable / incompatible
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const version = await invoke<string | null>(
          "check_app_update_available",
        );
        if (cancelled) return;
        if (version) {
          setAvailableVersion(version);
          setPhase("upgradable");
        } else {
          setPhase("incompatible");
        }
      } catch {
        // 检查失败（如离线）：仍允许尝试升级，避免完全卡死
        if (!cancelled) setPhase("upgradable");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    return () => {
      unlistenRef.current?.();
    };
  }, []);

  const startUpgrade = useCallback(async () => {
    setPhase("updating");
    setProgress(null);
    setErrorMsg(null);
    try {
      unlistenRef.current?.();
      unlistenRef.current = await listen<DownloadProgress>(
        "update-download-progress",
        (e) => setProgress(e.payload),
      );
      // 成功时后端会下载+安装+重启，不会返回；返回 false 表示无可用更新。
      const updating = await invoke<boolean>("install_update_and_restart");
      unlistenRef.current?.();
      unlistenRef.current = null;
      if (!updating) {
        // 竞态：检查时有更新、安装时已无 → 按不兼容处理
        setPhase("incompatible");
      }
      // updating === true：应用即将重启，保持 updating 态直到进程退出。
    } catch (e) {
      unlistenRef.current?.();
      unlistenRef.current = null;
      setErrorMsg(e instanceof Error ? e.message : String(e));
      setPhase("error");
    }
  }, []);

  const percent =
    progress && progress.total && progress.total > 0
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : null;
  const fmtMB = (n: number) => (n / 1024 / 1024).toFixed(1);

  const isIncompatible = phase === "incompatible";
  const accent = isIncompatible
    ? {
        chip: "bg-red-100 text-red-600 dark:bg-red-950/50 dark:text-red-400",
        Icon: AlertTriangle,
      }
    : {
        chip: "bg-amber-100 text-amber-600 dark:bg-amber-950/50 dark:text-amber-400",
        Icon: Database,
      };
  const AccentIcon = accent.Icon;

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-6 text-foreground">
      <div className="w-full max-w-lg space-y-5 rounded-2xl border border-border/60 bg-card/80 p-7 shadow-xl">
        <div className="flex items-start gap-4">
          <div
            className={`flex h-12 w-12 shrink-0 items-center justify-center rounded-xl ${accent.chip}`}
          >
            <AccentIcon className="h-6 w-6" />
          </div>
          <div className="space-y-1">
            <h1 className="text-lg font-semibold">
              {t("dbUpgrade.title", "数据库版本过新")}
            </h1>
            <p className="text-sm text-muted-foreground">
              {t(
                "dbUpgrade.description",
                "当前数据库由更新版本的 CC Switch 创建，需要升级应用后才能继续使用。升级不会删除你的数据。",
              )}
            </p>
            {dbVersion != null && supportedVersion != null && (
              <p className="pt-0.5 text-xs text-muted-foreground tabular-nums">
                {t("dbUpgrade.versionInfo", {
                  db: dbVersion,
                  supported: supportedVersion,
                  defaultValue: "数据库版本 v{{db}} · 应用支持 v{{supported}}",
                })}
              </p>
            )}
          </div>
        </div>

        {/* 错误详情 / 数据库路径 */}
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

        {phase === "checking" && (
          <p className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t("dbUpgrade.checking", "正在检查可用更新…")}
          </p>
        )}

        {phase === "upgradable" && availableVersion && (
          <p className="text-sm text-muted-foreground">
            {t("dbUpgrade.updateAvailable", {
              version: availableVersion,
              defaultValue: "发现新版本 v{{version}}，升级后即可继续使用。",
            })}
          </p>
        )}

        {phase === "incompatible" && (
          <div className="space-y-2 rounded-lg border border-red-300/60 bg-red-50 p-3 text-sm text-red-700 dark:border-red-500/40 dark:bg-red-950/40 dark:text-red-300">
            <p className="font-medium">
              {t("dbUpgrade.incompatibleTitle", "升级也无法解决")}
            </p>
            <p className="leading-relaxed">
              {t("dbUpgrade.incompatibleDescription", {
                db: dbVersion,
                supported: supportedVersion,
                defaultValue:
                  "你已是最新版本，但数据库版本（v{{db}}）仍高于本应用支持的版本（v{{supported}}）。该数据库可能由第三方客户端或更高版本创建，升级当前官方应用也无法兼容。",
              })}
            </p>
          </div>
        )}

        {phase === "updating" && (
          <div className="space-y-2">
            <div className="flex items-center justify-between text-sm">
              <span className="flex items-center gap-2 text-muted-foreground">
                <Loader2 className="h-4 w-4 animate-spin" />
                {percent === null
                  ? t("dbUpgrade.preparing", "正在准备更新…")
                  : t("dbUpgrade.downloading", "正在下载更新…")}
              </span>
              {percent !== null && (
                <span className="tabular-nums text-muted-foreground">
                  {percent}%
                </span>
              )}
            </div>
            <div className="h-2 w-full overflow-hidden rounded-full bg-muted">
              <div
                className={`h-full rounded-full bg-amber-500 transition-all duration-200 ${
                  percent === null ? "w-1/3 animate-pulse" : ""
                }`}
                style={percent === null ? undefined : { width: `${percent}%` }}
              />
            </div>
            {progress && (
              <p className="text-right text-xs tabular-nums text-muted-foreground">
                {fmtMB(progress.downloaded)} MB
                {progress.total ? ` / ${fmtMB(progress.total)} MB` : ""}
              </p>
            )}
          </div>
        )}

        {phase === "error" && errorMsg && (
          <p className="rounded-lg border border-red-300/60 bg-red-50 p-3 text-sm text-red-700 dark:border-red-500/40 dark:bg-red-950/40 dark:text-red-300">
            {errorMsg}
          </p>
        )}

        <div className="flex flex-wrap items-center gap-2">
          {(phase === "upgradable" || phase === "error") && (
            <Button
              onClick={startUpgrade}
              className="gap-2 bg-amber-500 text-white hover:bg-amber-600"
            >
              {phase === "error" ? (
                <RefreshCw className="h-4 w-4" />
              ) : (
                <Download className="h-4 w-4" />
              )}
              {phase === "error"
                ? t("dbUpgrade.retry", "重试升级")
                : t("dbUpgrade.upgradeNow", "升级应用")}
            </Button>
          )}

          {(phase === "incompatible" || phase === "error") && (
            <Button
              variant="outline"
              className="gap-2"
              onClick={() =>
                void invoke("open_external", { url: RELEASES_URL })
              }
            >
              <ExternalLink className="h-4 w-4" />
              {t("dbUpgrade.openReleases", "打开发布页")}
            </Button>
          )}

          <Button
            variant="outline"
            className="gap-2"
            onClick={() => void invoke("open_app_config_folder")}
            disabled={phase === "updating"}
          >
            <FolderOpen className="h-4 w-4" />
            {t("dbUpgrade.openConfigDir", "打开配置目录")}
          </Button>

          <Button
            variant="ghost"
            className="ml-auto text-muted-foreground"
            onClick={() => void exit(0)}
            disabled={phase === "updating"}
          >
            {t("dbUpgrade.quit", "退出")}
          </Button>
        </div>
      </div>
    </div>
  );
}

export default DatabaseUpgrade;
