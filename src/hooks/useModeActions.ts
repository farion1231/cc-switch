import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "@/lib/toast";
import type { AppId } from "@/lib/api";
import { providersApi } from "@/lib/api/providers";
import { proxyApi } from "@/lib/api/proxy";
import { proxyKeys } from "@/lib/query/proxy";
import type { AppMode } from "@/types/proxy";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";

/** 这几个客户端只在启动时读配置：改写了客户端文件要提示重启（Claude Code 会自己重读）。 */
const RESTART_TO_APPLY = new Set<AppId>([
  "codex",
  "gemini",
  "grokbuild",
  "claude-desktop",
]);

/** 进入 / 离开路由和聚合模式。两个方向都先在确认框里确认，出错由确认框就地显示。 */
export function useModeActions(app: AppId) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [pending, setPending] = useState(false);
  const appName = APP_DISPLAY_NAME[app];
  const restartHint = RESTART_TO_APPLY.has(app)
    ? t("mode.toast.restartHint", { app: appName })
    : "";

  const refresh = useCallback(async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: proxyKeys.status }),
      queryClient.invalidateQueries({ queryKey: proxyKeys.takeoverStatus }),
      queryClient.invalidateQueries({ queryKey: ["providers", app] }),
    ]);
    providersApi.updateTrayMenu().catch(() => undefined);
  }, [app, queryClient]);

  /** 进入路由 / 聚合模式；已经在另一种代理模式时后端一次写完，不经过直连。抛出错误给确认框显示。 */
  const enter = useCallback(
    async (
      mode: Exclude<AppMode, "direct">,
      route: string | null,
      routeName?: string,
    ) => {
      setPending(true);
      try {
        await proxyApi.setProxyTakeoverForApp(
          app,
          true,
          mode === "stack",
          route,
        );
        await refresh();
        toast.success(
          mode === "route"
            ? // Desktop 的模型列表也换成了路由那家的，要重启才看得到
              t("mode.toast.enteredRoute", {
                app: appName,
                provider: routeName ?? "",
              }) + (app === "claude-desktop" ? restartHint : "")
            : t(
                app === "codex"
                  ? "mode.toast.enteredStack"
                  : app === "claude-desktop"
                    ? "mode.toast.enteredStackDesktop"
                    : "mode.toast.enteredStackLive",
                { app: appName },
              ),
          { closeButton: true },
        );
      } finally {
        setPending(false);
      }
    },
    [app, appName, refresh, restartHint, t],
  );

  /**
   * 回到直连（确认框里确认后调用）。`useProviderId` 不为空时回到直连后改用这家
   * （「回到直连并使用」）。抛出错误给确认框显示。
   */
  const exitToDirect = useCallback(
    async ({
      directProviderId,
      useProviderId,
      providerName,
    }: {
      directProviderId: string | null;
      useProviderId?: string;
      providerName: string;
    }) => {
      setPending(true);
      try {
        await proxyApi.setProxyTakeoverForApp(app, false);
        if (useProviderId && useProviderId !== directProviderId) {
          await providersApi.switch(useProviderId, app);
        }
        await refresh();
        toast.success(
          [
            t("mode.toast.exited", { app: appName, provider: providerName }),
            restartHint,
          ]
            .filter(Boolean)
            .join(""),
          { closeButton: true },
        );
      } catch (error) {
        await refresh();
        throw error;
      } finally {
        setPending(false);
      }
    },
    [app, appName, refresh, restartHint, t],
  );

  return { enter, exitToDirect, pending, restartHint };
}
