import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { RefreshCw, ShieldCheck, Store, Wrench } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi } from "@/lib/api";
import type {
  CodexAuthRepairResult,
  CodexRepairStatus,
  CodexStoreRepairResult,
} from "@/lib/api/settings";

/**
 * CodexRepairSettings - Codex 修复工具
 *
 * 诊断并修复 Codex（OpenAI）的常见问题：
 * - 插件商店空白 / 插件装不了（curated 商店目录缺失损坏、config.toml 里被
 *   Codex 策略忽略的保留名市场注册）；
 * - 历史遗留的 0 字节 / 损坏 auth.json 导致一直停在登录页。
 *
 * 实现思路参考 CodexPlusPlus（BigPizzaV3/CodexPlusPlus）的插件市场修复。
 */
export function CodexRepairSettings() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<CodexRepairStatus | null>(null);
  const [isDiagnosing, setIsDiagnosing] = useState(false);
  const [isRepairingStore, setIsRepairingStore] = useState(false);
  const [isRepairingAuth, setIsRepairingAuth] = useState(false);
  const [showStoreConfirm, setShowStoreConfirm] = useState(false);

  const refresh = useCallback(async () => {
    setIsDiagnosing(true);
    try {
      setStatus(await settingsApi.codexRepairStatus());
    } catch (error) {
      console.error("Failed to diagnose Codex:", error);
      toast.error(t("settings.codexRepair.diagnoseFailed"));
    } finally {
      setIsDiagnosing(false);
    }
  }, [t]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleRepairStore = async () => {
    setShowStoreConfirm(false);
    setIsRepairingStore(true);
    try {
      const result: CodexStoreRepairResult =
        await settingsApi.repairCodexPluginStore();
      toast.success(result.message || t("settings.codexRepair.repairStoreDone"));
      setStatus(await settingsApi.codexRepairStatus());
    } catch (error) {
      console.error("Failed to repair Codex plugin store:", error);
      toast.error(t("settings.codexRepair.repairStoreFailed"), {
        description: String(error),
      });
    } finally {
      setIsRepairingStore(false);
    }
  };

  const handleRepairAuth = async () => {
    setIsRepairingAuth(true);
    try {
      const result: CodexAuthRepairResult =
        await settingsApi.repairCodexAuthJson();
      toast.success(result.message || t("settings.codexRepair.repairAuthDone"));
      setStatus(await settingsApi.codexRepairStatus());
    } catch (error) {
      console.error("Failed to repair Codex auth.json:", error);
      toast.error(t("settings.codexRepair.repairAuthFailed"), {
        description: String(error),
      });
    } finally {
      setIsRepairingAuth(false);
    }
  };

  const storeBadge = status ? (
    status.curatedStore.valid ? (
      <Badge className="bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/30">
        {t("settings.codexRepair.storeOk", {
          count: status.curatedStore.pluginCount,
        })}
      </Badge>
    ) : (
      <Badge className="bg-red-500/10 text-red-600 dark:text-red-400 border-red-500/30">
        {status.curatedStore.exists
          ? t("settings.codexRepair.storeCorrupt")
          : t("settings.codexRepair.storeMissing")}
      </Badge>
    )
  ) : null;

  const configBadge = status ? (
    status.config.parseError ? (
      <Badge className="bg-red-500/10 text-red-600 dark:text-red-400 border-red-500/30">
        {t("settings.codexRepair.configParseError")}
      </Badge>
    ) : status.config.blockedMarketplaces.length > 0 ? (
      <Badge className="bg-amber-500/10 text-amber-600 dark:text-amber-400 border-amber-500/30">
        {t("settings.codexRepair.blockedCount", {
          count: status.config.blockedMarketplaces.length,
        })}
      </Badge>
    ) : (
      <Badge className="bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/30">
        {t("settings.codexRepair.configOk")}
      </Badge>
    )
  ) : null;

  const authBadge = status ? (
    !status.auth.exists ? (
      <Badge className="bg-muted text-muted-foreground border-border/60">
        {t("settings.codexRepair.authMissing")}
      </Badge>
    ) : status.auth.validJson ? (
      <Badge className="bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/30">
        {t("settings.codexRepair.authOk")}
      </Badge>
    ) : (
      <Badge className="bg-red-500/10 text-red-600 dark:text-red-400 border-red-500/30">
        {t("settings.codexRepair.authCorrupt")}
      </Badge>
    )
  ) : null;

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <Wrench className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">{t("settings.codexRepair.title")}</h3>
        {status &&
          (status.needsRepair ? (
            <Badge className="bg-amber-500/10 text-amber-600 dark:text-amber-400 border-amber-500/30">
              {t("settings.codexRepair.needsRepair")}
            </Badge>
          ) : (
            <Badge className="bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/30">
              {t("settings.codexRepair.allOk")}
            </Badge>
          ))}
        <Button
          variant="ghost"
          size="sm"
          className="ml-auto h-7 gap-1 text-xs"
          onClick={() => void refresh()}
          disabled={isDiagnosing}
        >
          <RefreshCw
            className={`h-3.5 w-3.5 ${isDiagnosing ? "animate-spin" : ""}`}
          />
          {t("settings.codexRepair.diagnose")}
        </Button>
      </div>

      <p className="text-xs text-muted-foreground">
        {t("settings.codexRepair.description")}
      </p>

      {status && (
        <div className="space-y-3 rounded-lg border border-border/40 p-3 text-xs">
          <div className="flex items-center justify-between gap-2">
            <span className="text-muted-foreground">
              {t("settings.codexRepair.codexHome")}
            </span>
            <span className="font-mono text-foreground/80 truncate">
              {status.codexHome}
            </span>
          </div>

          <div className="flex items-center justify-between gap-2">
            <span className="flex items-center gap-1.5 text-muted-foreground">
              <Store className="h-3.5 w-3.5" />
              {t("settings.codexRepair.storeStatus")}
            </span>
            {storeBadge}
          </div>

          <div className="flex items-center justify-between gap-2">
            <span className="flex items-center gap-1.5 text-muted-foreground">
              <ShieldCheck className="h-3.5 w-3.5" />
              {t("settings.codexRepair.configStatus")}
            </span>
            {configBadge}
          </div>

          {status.config.blockedMarketplaces.length > 0 && (
            <ul className="space-y-1 pl-5 list-disc text-muted-foreground">
              {status.config.blockedMarketplaces.map((entry) => (
                <li key={`${entry.name}-${entry.source}`}>
                  <span className="font-mono text-foreground/80">
                    {entry.name}
                  </span>{" "}
                  → <span className="font-mono">{entry.source}</span>
                </li>
              ))}
            </ul>
          )}

          {status.config.parseError && (
            <p className="text-red-600 dark:text-red-400 font-mono break-all">
              {status.config.parseError}
            </p>
          )}

          <div className="flex items-center justify-between gap-2">
            <span className="flex items-center gap-1.5 text-muted-foreground">
              <ShieldCheck className="h-3.5 w-3.5" />
              {t("settings.codexRepair.authStatus")}
            </span>
            {authBadge}
          </div>
        </div>
      )}

      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          onClick={() => setShowStoreConfirm(true)}
          disabled={isRepairingStore || isRepairingAuth}
        >
          <Store className="h-4 w-4 mr-1.5" />
          {isRepairingStore
            ? t("settings.codexRepair.repairingStore")
            : t("settings.codexRepair.repairStore")}
        </Button>
        <Button
          size="sm"
          variant="outline"
          onClick={() => void handleRepairAuth()}
          disabled={isRepairingStore || isRepairingAuth}
        >
          <ShieldCheck className="h-4 w-4 mr-1.5" />
          {isRepairingAuth
            ? t("settings.codexRepair.repairingAuth")
            : t("settings.codexRepair.repairAuth")}
        </Button>
      </div>

      <ConfirmDialog
        isOpen={showStoreConfirm}
        variant="info"
        title={t("settings.codexRepair.repairStoreConfirmTitle")}
        message={t("settings.codexRepair.repairStoreConfirmMessage")}
        confirmText={t("settings.codexRepair.repairStore")}
        onConfirm={() => void handleRepairStore()}
        onCancel={() => setShowStoreConfirm(false)}
      />
    </section>
  );
}
