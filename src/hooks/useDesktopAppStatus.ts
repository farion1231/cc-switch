import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { desktopAppApi, type DesktopAppStatus } from "@/lib/api/desktopApp";
import type { AppId } from "@/lib/api";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { toast } from "@/lib/toast";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isLinux, isMac, isWindows } from "@/lib/platform";

export const DESKTOP_APPS = ["codex", "claude-desktop", "opencode"] as const;

export type DesktopApp = (typeof DESKTOP_APPS)[number];

export function isDesktopApp(app: AppId): app is DesktopApp {
  return (DESKTOP_APPS as readonly string[]).includes(app);
}

export const desktopAppKeys = {
  status: (app: AppId) => ["desktop-app-status", app] as const,
};

const STATUS_REFETCH_INTERVAL = 5_000;

export function useDesktopAppStatus(app: AppId) {
  const supported = isDesktopApp(app) && (isWindows() || isMac() || isLinux());
  const queryClient = useQueryClient();
  const { t } = useTranslation();
  const name = APP_DISPLAY_NAME[app];

  const statusQuery = useQuery<DesktopAppStatus>({
    queryKey: desktopAppKeys.status(app),
    queryFn: () => desktopAppApi.getStatus(app),
    enabled: supported,
    staleTime: STATUS_REFETCH_INTERVAL,
    refetchInterval: STATUS_REFETCH_INTERVAL,
    retry: false,
  });

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: desktopAppKeys.status(app) });

  const launchMutation = useMutation({
    mutationFn: () => desktopAppApi.launch(app),
    onSuccess: () => {
      toast.success(t("appPage.desktopApp.started", { name }));
      void invalidate();
    },
    onError: (error) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(t("appPage.desktopApp.startFailed", { name, error: detail }));
    },
  });

  const restartMutation = useMutation({
    mutationFn: () => desktopAppApi.restart(app),
    onSuccess: () => {
      toast.success(t("appPage.desktopApp.restarted", { name }));
      void invalidate();
    },
    onError: (error) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("appPage.desktopApp.restartFailed", { name, error: detail }),
      );
    },
  });

  const status = statusQuery.data;
  const installed = status?.installed === true;
  const running = status?.running === true;

  return {
    supported,
    checking: statusQuery.isPending,
    installed,
    running,
    launch: launchMutation.mutate,
    restart: restartMutation.mutate,
    launching: launchMutation.isPending,
    restarting: restartMutation.isPending,
  };
}
