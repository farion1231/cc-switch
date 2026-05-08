/**
 * 代理配置管理 Hook
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import type { ProxyConfig } from "@/types/proxy";

/**
 * 代理配置管理
 */
export function useProxyConfig() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  // 查询配置
  const { data: config, isLoading } = useQuery({
    queryKey: ["proxyConfig"],
    queryFn: () => invoke<ProxyConfig>("get_proxy_config"),
  });

  // 更新配置
  const updateMutation = useMutation({
    mutationFn: (newConfig: ProxyConfig) =>
      invoke("update_proxy_config", { config: newConfig }),
    onSuccess: () => {
      toast.success(t("proxy.settings.toast.saved"), { closeButton: true });
      queryClient.invalidateQueries({ queryKey: ["proxyConfig"] });
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
    },
    onError: (error: Error) => {
      toast.error(
        t("proxy.settings.toast.saveFailed", {
          error: error.message,
        }),
      );
    },
  });

  return {
    config,
    isLoading,
    updateConfig: updateMutation.mutateAsync,
    isUpdating: updateMutation.isPending,
  };
}
