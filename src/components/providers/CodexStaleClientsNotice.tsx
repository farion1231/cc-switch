import { useState } from "react";
import { RotateCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  useAcknowledgeCodexStaleClients,
  useRestartCodexAppServerDaemon,
} from "@/lib/query/proxy";
import type { CodexStaleClients } from "@/types/proxy";

const DISMISSED_STORAGE_KEY = "cc-switch:codex-stale-clients-notice-dismissed";

function readDismissedPreference() {
  try {
    return window.localStorage.getItem(DISMISSED_STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

interface CodexStaleClientsNoticeProps {
  staleClients: CodexStaleClients;
  onDismiss?: () => void;
}

/**
 * Codex 客户端可能缓存着旧账号或模型列表（模型目录只在启动时读，切换账号或直连、路由、聚合
 * 模式后都会变）。命令行连的守护进程确认后一键重启；桌面版、编辑器插件只提示用户彻底退出再
 * 开。重启会中断守护进程里正在运行的任务，执行期间确认框保持打开。看不到桌面版、编辑器插件
 * 进程时（Windows），只能说「可能」，关掉提示时让后端记下现在这份，同一份不再提示。
 */
export function CodexStaleClientsNotice({
  staleClients,
  onDismiss,
}: CodexStaleClientsNoticeProps) {
  const { t } = useTranslation();
  const [confirming, setConfirming] = useState(false);
  const [dismissed, setDismissed] = useState(readDismissedPreference);
  const [dontShowAgain, setDontShowAgain] = useState(false);
  const restart = useRestartCodexAppServerDaemon();
  const acknowledge = useAcknowledgeCodexStaleClients();
  const dismiss = () => {
    if (dontShowAgain) {
      try {
        window.localStorage.setItem(DISMISSED_STORAGE_KEY, "true");
      } catch {
        // 存储不可用时仍关闭本次提醒，不影响账号或模型切换。
      }
      setDismissed(true);
    }
    if (staleClients.unverified) acknowledge.mutate();
    onDismiss?.();
  };

  if (dismissed) return null;

  return (
    <>
      <Notice
        tone="warning"
        title={t(
          staleClients.unverified
            ? "proxy.stackMode.codexStale.unverifiedTitle"
            : staleClients.auth
              ? "proxy.stackMode.codexStale.authTitle"
              : "proxy.stackMode.codexStale.title",
        )}
        onDismiss={dismiss}
        dismissLabel={t("common.close")}
        actions={
          staleClients.daemon ? (
            <Button
              variant="neutral"
              size="compact"
              disabled={restart.isPending}
              onClick={() => setConfirming(true)}
            >
              <RotateCw
                className={`h-3.5 w-3.5 ${restart.isPending ? "animate-spin" : ""}`}
              />
              {t("proxy.stackMode.codexStale.restart")}
            </Button>
          ) : undefined
        }
      >
        {staleClients.daemon && (
          <span className="block">
            {t("proxy.stackMode.codexStale.daemon")}
          </span>
        )}
        {staleClients.others && (
          <span className="block">
            {t("proxy.stackMode.codexStale.others")}
          </span>
        )}
        {staleClients.unverified && (
          <span className="block">
            {t("proxy.stackMode.codexStale.unverifiedHint")}
          </span>
        )}
        <label className="mt-2 flex items-center gap-2">
          <input
            type="checkbox"
            checked={dontShowAgain}
            onChange={(event) => setDontShowAgain(event.target.checked)}
          />
          {t("proxy.stackMode.codexStale.dontShowAgain")}
        </label>
      </Notice>
      <ConfirmDialog
        isOpen={confirming}
        title={t("proxy.stackMode.codexStale.confirmTitle")}
        message={t("proxy.stackMode.codexStale.confirmMessage")}
        confirmText={t("proxy.stackMode.codexStale.confirm")}
        pending={restart.isPending}
        onConfirm={() =>
          restart.mutate(undefined, {
            onSettled: () => setConfirming(false),
          })
        }
        onCancel={() => setConfirming(false)}
      />
    </>
  );
}
