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
}

export const failoverApi = {
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
