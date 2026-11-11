import { invoke } from "@tauri-apps/api/core";
import type { AuxiliaryConfig, AuxiliaryQueueItem } from "@/types/proxy";
import type { Provider } from "@/lib/api/failover";

export const auxiliaryApi = {
  // 获取辅助请求队列
  async getAuxiliaryQueue(appType: string): Promise<AuxiliaryQueueItem[]> {
    return invoke("get_auxiliary_queue", { appType });
  },

  // 获取可添加到队列的供应商（不在队列中的）
  async getAvailableProvidersForAuxiliary(
    appType: string,
  ): Promise<Provider[]> {
    return invoke("get_available_providers_for_auxiliary", { appType });
  },

  // 添加供应商到辅助请求队列
  async addToAuxiliaryQueue(
    appType: string,
    providerId: string,
  ): Promise<void> {
    return invoke("add_to_auxiliary_queue", { appType, providerId });
  },

  // 从辅助请求队列移除供应商
  async removeFromAuxiliaryQueue(
    appType: string,
    providerId: string,
  ): Promise<void> {
    return invoke("remove_from_auxiliary_queue", { appType, providerId });
  },

  // 按拖拽结果重排辅助请求队列
  async reorderAuxiliaryQueue(
    appType: string,
    providerIds: string[],
  ): Promise<void> {
    return invoke("reorder_auxiliary_queue", { appType, providerIds });
  },

  // 设置队列条目的出站模型名覆写（null / 空 = 透传客户端模型）
  async setAuxiliaryModel(
    appType: string,
    providerId: string,
    model: string | null,
  ): Promise<void> {
    return invoke("set_auxiliary_model", { appType, providerId, model });
  },

  // 读取辅助请求队列的两个开关
  async getAuxiliaryConfig(appType: string): Promise<AuxiliaryConfig> {
    return invoke("get_auxiliary_config", { appType });
  },

  // 写入辅助请求队列的两个开关
  async setAuxiliaryConfig(
    appType: string,
    config: AuxiliaryConfig,
  ): Promise<void> {
    return invoke("set_auxiliary_config", { appType, config });
  },
};
