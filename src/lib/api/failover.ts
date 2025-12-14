import { invoke } from "@tauri-apps/api/core";
import type {
  ProviderHealth,
  CircuitBreakerConfig,
  CircuitBreakerStats,
  FailoverQueueItem,
} from "@/types/proxy";

export interface Provider {
  id: string;
  name: string;
  settingsConfig: unknown;
  websiteUrl?: string;
  category?: string;
  createdAt?: number;
  sortIndex?: number;
  notes?: string;
  meta?: unknown;
  icon?: string;
  iconColor?: string;
  isProxyTarget?: boolean;
}

export const failoverApi = {
  // ========== 旧版代理目标 API（保留向后兼容）==========

  // 获取代理目标列表
  async getProxyTargets(appType: string): Promise<Provider[]> {
    return invoke("get_proxy_targets", { appType });
  },

  // 设置代理目标
  async setProxyTarget(
    providerId: string,
    appType: string,
    enabled: boolean,
  ): Promise<void> {
    return invoke("set_proxy_target", { providerId, appType, enabled });
  },

  // ========== 熔断器 API ==========

  // 获取供应商健康状态
  async getProviderHealth(
    providerId: string,
    appType: string,
  ): Promise<ProviderHealth> {
    return invoke("get_provider_health", { providerId, appType });
  },

  // 重置熔断器
  async resetCircuitBreaker(
    providerId: string,
    appType: string,
  ): Promise<void> {
    return invoke("reset_circuit_breaker", { providerId, appType });
  },

  // 获取熔断器配置
  async getCircuitBreakerConfig(): Promise<CircuitBreakerConfig> {
    return invoke("get_circuit_breaker_config");
  },

  // 更新熔断器配置
  async updateCircuitBreakerConfig(
    config: CircuitBreakerConfig,
  ): Promise<void> {
    return invoke("update_circuit_breaker_config", { config });
  },

  // 获取熔断器统计信息
  async getCircuitBreakerStats(
    providerId: string,
    appType: string,
  ): Promise<CircuitBreakerStats | null> {
    return invoke("get_circuit_breaker_stats", { providerId, appType });
  },

  // ========== 故障转移队列 API（新） ==========

  // 获取故障转移队列
  async getFailoverQueue(appType: string): Promise<FailoverQueueItem[]> {
    return invoke("get_failover_queue", { appType });
  },

  // 获取可添加到队列的供应商（不在队列中的）
  async getAvailableProvidersForFailover(appType: string): Promise<Provider[]> {
    return invoke("get_available_providers_for_failover", { appType });
  },

  // 添加供应商到故障转移队列
  async addToFailoverQueue(
    appType: string,
    providerId: string,
  ): Promise<void> {
    return invoke("add_to_failover_queue", { appType, providerId });
  },

  // 从故障转移队列移除供应商
  async removeFromFailoverQueue(
    appType: string,
    providerId: string,
  ): Promise<void> {
    return invoke("remove_from_failover_queue", { appType, providerId });
  },

  // 重新排序故障转移队列
  async reorderFailoverQueue(
    appType: string,
    providerIds: string[],
  ): Promise<void> {
    return invoke("reorder_failover_queue", { appType, providerIds });
  },

  // 设置故障转移队列项的启用状态
  async setFailoverItemEnabled(
    appType: string,
    providerId: string,
    enabled: boolean,
  ): Promise<void> {
    return invoke("set_failover_item_enabled", { appType, providerId, enabled });
  },
};
