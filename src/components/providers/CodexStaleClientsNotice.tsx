import { useEffect, useId, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import {
  proxyKeys,
  useAcknowledgeCodexStaleClients,
  useRestartCodexAppServerDaemon,
} from "@/lib/query/proxy";
import type { CodexStaleClients } from "@/types/proxy";

interface CodexStaleClientsNoticeProps {
  staleClients: CodexStaleClients;
  onDismiss?: () => void;
}

/**
 * 把 CLI 守护进程的重启入口放在通知里。通知中说明会中断任务，只有点击才执行。
 * 使用 Sonner 的有限时长和悬停计时：移出后继续倒计时，点击后立即收起。
 * 自动消失只隐藏这一代提示，不把它当作用户确认客户端已更新。
 */
export function CodexStaleClientsNotice({
  staleClients: { daemon, others, auth, unverified },
  onDismiss,
}: CodexStaleClientsNoticeProps) {
  const { t } = useTranslation();
  const id = useId();
  const queryClient = useQueryClient();
  const { mutate: restart } = useRestartCodexAppServerDaemon();
  const { mutate: acknowledge } = useAcknowledgeCodexStaleClients();
  const onDismissRef = useRef(onDismiss);
  useEffect(() => {
    onDismissRef.current = onDismiss;
  }, [onDismiss]);

  useEffect(() => {
    let active = true;
    let dismissed = false;
    const dismiss = (explicit = false) => {
      if (!active || dismissed) return;
      dismissed = true;
      if (explicit && unverified) acknowledge();
      onDismissRef.current?.();
    };

    // 全局 Toaster 在 App 后挂载；等本轮 effects 完成，避免缓存命中时首条通知丢失。
    queueMicrotask(() => {
      if (!active) return;
      toast.warning(
        t(
          unverified
            ? "proxy.stackMode.codexStale.unverifiedTitle"
            : auth
              ? "proxy.stackMode.codexStale.authTitle"
              : "proxy.stackMode.codexStale.title",
        ),
        {
          id,
          duration: 8000,
          closeButton: true,
          description: (
            <>
              {daemon && (
                <span className="block">
                  {t("proxy.stackMode.codexStale.restartWarning")}
                </span>
              )}
              {others && (
                <span className="block">
                  {t("proxy.stackMode.codexStale.others")}
                </span>
              )}
              {unverified && (
                <span className="block">
                  {t("proxy.stackMode.codexStale.unverifiedHint")}
                </span>
              )}
            </>
          ),
          action: daemon
            ? {
                label: t("proxy.stackMode.codexStale.restart"),
                onClick: (event) => {
                  if (!active || dismissed) return;
                  // 新目录的通知可能在上一次重启期间出现；跨组件挂载也只重启一次。
                  if (
                    queryClient.isMutating({
                      mutationKey: proxyKeys.codexRestart,
                    }) > 0
                  ) {
                    event.preventDefault();
                    toast.info(t("proxy.stackMode.codexStale.restarting"));
                    return;
                  }
                  dismiss();
                  toast.dismiss(id);
                  restart();
                },
              }
            : undefined,
          onDismiss: () => dismiss(true),
          onAutoClose: () => dismiss(),
        },
      );
    });

    return () => {
      // 卸载、切换页面、切换配置时撤掉旧按钮，不确认或隐藏下一代提示。
      active = false;
      toast.dismiss(id);
    };
  }, [
    id,
    t,
    daemon,
    others,
    auth,
    unverified,
    restart,
    acknowledge,
    queryClient,
  ]);

  return null;
}
