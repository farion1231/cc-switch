import { useState } from "react";
import { RotateCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { useRestartCodexAppServerDaemon } from "@/lib/query/proxy";
import type { CodexStaleClients } from "@/types/proxy";

interface CodexStaleClientsNoticeProps {
  staleClients: CodexStaleClients;
  onDismiss?: () => void;
}

/**
 * Stack 模式下 Codex 客户端还在用旧的模型列表（它们只在启动时读模型目录）。命令行连的守护
 * 进程确认后一键重启；桌面版、编辑器插件只提示用户彻底退出再开。重启会中断守护进程里正在
 * 运行的任务，执行期间确认框保持打开。
 */
export function CodexStaleClientsNotice({
  staleClients,
  onDismiss,
}: CodexStaleClientsNoticeProps) {
  const { t } = useTranslation();
  const [confirming, setConfirming] = useState(false);
  const restart = useRestartCodexAppServerDaemon();

  return (
    <>
      <Notice
        tone="warning"
        title={t("proxy.stackMode.codexStale.title")}
        onDismiss={onDismiss}
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
