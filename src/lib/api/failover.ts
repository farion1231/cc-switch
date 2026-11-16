import { invoke } from "@tauri-apps/api/core";
import type { ProviderHealth, FailoverQueueItem } from "@/types/proxy";

export const failoverApi = {
  // ========== 熔断器 API ==========

  // 获取供应商健康状态
  async getProviderHealth(
    providerId: string,
    appType: string,
  ): Promise<ProviderHealth> {
    return invoke("get_provider_health", { providerId, appType });
  },

  // ========== 故障转移队列 API（新） ==========

  // 获取故障转移队列
  async getFailoverQueue(appType: string): Promise<FailoverQueueItem[]> {
    return invoke("get_failover_queue", { appType });
  },

  // 添加供应商到故障转移队列
  async addToFailoverQueue(appType: string, providerId: string): Promise<void> {
    return invoke("add_to_failover_queue", { appType, providerId });
  },

  // 从故障转移队列移除供应商
  async removeFromFailoverQueue(
    appType: string,
    providerId: string,
  ): Promise<void> {
    return invoke("remove_from_failover_queue", { appType, providerId });
  },

  // 获取指定应用的自动故障转移开关状态
  async getAutoFailoverEnabled(appType: string): Promise<boolean> {
    return invoke("get_auto_failover_enabled", { appType });
  },

  // 设置指定应用的自动故障转移开关状态
  async setAutoFailoverEnabled(
    appType: string,
    enabled: boolean,
  ): Promise<void> {
    return invoke("set_auto_failover_enabled", { appType, enabled });
  },
};
