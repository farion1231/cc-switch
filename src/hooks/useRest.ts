/**
 * REST 控制面 React Hooks（镜像 useGlobalProxy 范式）
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "@/lib/toast";
import { useTranslation } from "react-i18next";
import {
  getRestConfig,
  setRestConfig,
  regenerateRestToken,
  probeRestAvailable,
  type RestConfig,
} from "@/lib/api/rest";

/** 获取 REST 配置 */
export function useRestConfig() {
  return useQuery({
    queryKey: ["restConfig"],
    queryFn: getRestConfig,
    staleTime: 30 * 1000,
  });
}

/** 设置 REST 配置 */
export function useSetRestConfig() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: setRestConfig,
    onSuccess: (cfg: RestConfig) => {
      toast.success(t("settings.restApi.saved"));
      queryClient.setQueryData(["restConfig"], cfg);
    },
    onError: (error: unknown) => {
      const message =
        error instanceof Error
          ? error.message
          : typeof error === "string"
            ? error
            : "Unknown error";
      toast.error(t("settings.restApi.saveFailed", { error: message }));
    },
  });
}

/** 重新生成 token */
export function useRegenerateToken() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: regenerateRestToken,
    onSuccess: (cfg: RestConfig) => {
      toast.success(t("settings.restApi.tokenRegenerated"));
      queryClient.setQueryData(["restConfig"], cfg);
    },
    onError: (error: unknown) => {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(t("settings.restApi.regenerateFailed", { error: message }));
    },
  });
}

/** 探测 REST 是否可用（feature on） */
export function useRestAvailable() {
  return useQuery({
    queryKey: ["restAvailable"],
    queryFn: probeRestAvailable,
    staleTime: Infinity,
    retry: false,
  });
}
