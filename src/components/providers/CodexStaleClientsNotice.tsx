import { useState } from "react";
import { AlertTriangle, RotateCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { useRestartCodexAppServerDaemon } from "@/lib/query/proxy";
import type { CodexStaleClients } from "@/types/proxy";

interface CodexStaleClientsNoticeProps {
  staleClients: CodexStaleClients;
}

/**
 * Stack 模式下 Codex 客户端还在用旧的模型列表（它们只在启动时读模型目录）。命令行连的守护
 * 进程确认后一键重启；桌面版、编辑器插件只提示用户彻底退出再开。重启会中断守护进程里正在
 * 运行的任务，执行期间确认框保持打开。
 */
export function CodexStaleClientsNotice({
  staleClients,
}: CodexStaleClientsNoticeProps) {
  const { t } = useTranslation();
  const [confirming, setConfirming] = useState(false);
  const restart = useRestartCodexAppServerDaemon();

  return (
    <div
      role="alert"
      className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm text-amber-900 dark:text-amber-200"
    >
      <div className="flex items-center gap-2 font-medium">
        <AlertTriangle className="h-4 w-4 shrink-0" />
        {t("proxy.stackMode.codexStale.title")}
      </div>
      <ul className="mt-2 space-y-1 text-xs leading-relaxed">
        {staleClients.daemon && (
          <li>{t("proxy.stackMode.codexStale.daemon")}</li>
        )}
        {staleClients.others && (
          <li>{t("proxy.stackMode.codexStale.others")}</li>
        )}
      </ul>
      {staleClients.daemon && (
        <Button
          size="sm"
          variant="outline"
          className="mt-3 gap-1.5"
          disabled={restart.isPending}
          onClick={() => setConfirming(true)}
        >
          <RotateCw
            className={`h-3.5 w-3.5 ${restart.isPending ? "animate-spin" : ""}`}
          />
          {t("proxy.stackMode.codexStale.restart")}
        </Button>
      )}
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
    </div>
  );
}
