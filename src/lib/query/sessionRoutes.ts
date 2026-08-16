import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { sessionRoutesApi, type SessionRoutingConfig } from "@/lib/api/sessionRoutes";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { extractErrorMessage } from "@/utils/errorUtils";

export const sessionRouteKeys = {
  all: ["sessionRoutes"] as const,
  config: (appType: string) => ["sessionRoutes", "config", appType] as const,
  routes: (appType: string) => ["sessionRoutes", "routes", appType] as const,
  load: (appType: string) => ["sessionRoutes", "load", appType] as const,
};

export function useSessionRoutingConfig(appType: string) {
  return useQuery({
    queryKey: sessionRouteKeys.config(appType),
    queryFn: () => sessionRoutesApi.getConfig(appType),
    enabled: !!appType,
  });
}

export function useUpdateSessionRoutingConfig() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: ({
      appType,
      config,
    }: {
      appType: string;
      config: SessionRoutingConfig;
    }) => sessionRoutesApi.updateConfig(appType, config),
    onSuccess: (_, { appType }) => {
      queryClient.invalidateQueries({
        queryKey: sessionRouteKeys.config(appType),
      });
      toast.success(t("sessionRoutes.configUpdated"));
    },
    onError: (error) => {
      console.error("[SessionRoutes] 更新配置失败:", error);
      toast.error(`更新配置失败: ${extractErrorMessage(error)}`);
    },
  });
}

export function useActiveSessionRoutes(appType: string) {
  return useQuery({
    queryKey: sessionRouteKeys.routes(appType),
    queryFn: () => sessionRoutesApi.getActiveRoutes(appType),
    enabled: !!appType,
    refetchInterval: 10000, // 每 10 秒刷新
  });
}

export function useDeleteSessionRoute() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      sessionId,
      appType,
    }: {
      sessionId: string;
      appType: string;
    }) => sessionRoutesApi.deleteRoute(sessionId, appType),
    onSuccess: (_, { appType }) => {
      queryClient.invalidateQueries({
        queryKey: sessionRouteKeys.routes(appType),
      });
      queryClient.invalidateQueries({
        queryKey: sessionRouteKeys.load(appType),
      });
    },
  });
}

export function useSessionProviderLoad(appType: string) {
  return useQuery({
    queryKey: sessionRouteKeys.load(appType),
    queryFn: () => sessionRoutesApi.getProviderLoad(appType),
    enabled: !!appType,
    refetchInterval: 10000,
  });
}

export function useCleanupExpiredRoutes() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: ({
      appType,
      ttlSeconds,
    }: {
      appType: string;
      ttlSeconds: number;
    }) => sessionRoutesApi.cleanupExpired(appType, ttlSeconds),
    onSuccess: (count, { appType }) => {
      queryClient.invalidateQueries({
        queryKey: sessionRouteKeys.routes(appType),
      });
      queryClient.invalidateQueries({
        queryKey: sessionRouteKeys.load(appType),
      });
      toast.success(
        t("sessionRoutes.cleanedUp", { count }),
      );
    },
    onError: (error) => {
      toast.error(extractErrorMessage(error));
    },
  });
}