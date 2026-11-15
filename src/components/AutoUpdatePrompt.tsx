import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useUpdate } from "@/contexts/UpdateContext";
import { settingsApi } from "@/lib/api";
import { toast } from "@/lib/toast";
import { extractErrorMessage } from "@/utils/errorUtils";

// 点了「稍后」的版本记在本机：同一版本不再弹，侧栏红点、关于页按钮和托盘菜单里仍能安装
const DISMISSED_KEY = "ccswitch:autoUpdate:promptDismissedVersion";
// 提示挂着时多久重取一次路由状态（本机 IPC，开销可忽略）
const ROUTING_REFRESH_MS = 5000;

function readDismissed(): string | null {
  try {
    return localStorage.getItem(DISMISSED_KEY);
  } catch {
    return null;
  }
}

/**
 * 自动更新在后台下好新版本后，主界面内容区右下角的安装提示。
 * 装不装由用户点：CC Switch 一重启，经本地路由的请求会被切断。
 */
export function AutoUpdatePrompt() {
  const { t } = useTranslation();
  const { readyUpdate, routingInUse, refreshAutoUpdate, resetDismiss } =
    useUpdate();
  const [dismissedVersion, setDismissedVersion] = useState(readDismissed);
  const [installing, setInstalling] = useState(false);

  const version = readyUpdate?.version;
  const visible = !!version && dismissedVersion !== version;

  // 显示期间定时重取状态：用户可能在提示挂着时才开路由，「请求会中断」的警告要跟上
  useEffect(() => {
    if (!visible) return;
    void refreshAutoUpdate();
    const timer = window.setInterval(
      () => void refreshAutoUpdate(),
      ROUTING_REFRESH_MS,
    );
    return () => window.clearInterval(timer);
  }, [visible, refreshAutoUpdate]);

  const handleLater = useCallback(() => {
    if (!version) return;
    setDismissedVersion(version);
    try {
      localStorage.setItem(DISMISSED_KEY, version);
    } catch {
      // 存不下就只在本次运行里不再弹
    }
  }, [version]);

  const handleInstall = useCallback(async () => {
    setInstalling(true);
    try {
      resetDismiss();
      const installed = await settingsApi.installUpdateAndRestart();
      if (!installed) {
        // 服务器上已经没有更新了（版本被撤回）：后端不再报这个包，提示随之收起
        setInstalling(false);
        await refreshAutoUpdate();
      }
    } catch (error) {
      console.error("[AutoUpdatePrompt] Install failed", error);
      toast.error(t("autoUpdate.installFailed"), {
        description: extractErrorMessage(error) || undefined,
        closeButton: true,
      });
      setInstalling(false);
    }
  }, [refreshAutoUpdate, resetDismiss, t]);

  if (!visible) return null;

  return (
    <div
      role="status"
      aria-live="polite"
      className="absolute bottom-4 end-4 z-30 w-[264px] max-w-[calc(100%-2rem)] rounded-panel border border-border bg-surface px-3 py-2.5 shadow-v7-lg duration-200 animate-in fade-in slide-in-from-bottom-2"
    >
      <div className="flex items-start gap-2">
        <Download className="mt-0.5 h-3.5 w-3.5 shrink-0 text-action-text" />
        <div className="min-w-0 flex-1 space-y-0.5">
          <p className="text-body font-medium text-fg-1">
            {t("autoUpdate.readyTitle", { version })}
          </p>
          <p className="text-caption text-fg-2">
            {routingInUse
              ? t("autoUpdate.readyBodyRouting")
              : t("autoUpdate.readyBody")}
          </p>
        </div>
      </div>
      <div className="mt-2 flex justify-end gap-1.5">
        <Button
          type="button"
          variant="neutral"
          size="compact"
          onClick={handleLater}
          disabled={installing}
        >
          {t("autoUpdate.later")}
        </Button>
        <Button
          type="button"
          variant="solid"
          size="compact"
          onClick={() => void handleInstall()}
          disabled={installing}
        >
          {installing ? (
            <>
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              {t("settings.updating")}
            </>
          ) : (
            t("autoUpdate.installNow")
          )}
        </Button>
      </div>
    </div>
  );
}
