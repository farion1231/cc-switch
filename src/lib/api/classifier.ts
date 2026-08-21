import { invoke } from "@tauri-apps/api/core";
import type { ClassifierConfig, ClassifierQueueItem } from "@/types/proxy";
import type { Provider } from "@/lib/api/failover";

export const classifierApi = {
  // 获取分类器队列
  async getClassifierQueue(appType: string): Promise<ClassifierQueueItem[]> {
    return invoke("get_classifier_queue", { appType });
  },

  // 获取可添加到队列的供应商（不在队列中的）
  async getAvailableProvidersForClassifier(
    appType: string,
  ): Promise<Provider[]> {
    return invoke("get_available_providers_for_classifier", { appType });
  },

  // 添加供应商到分类器队列
  async addToClassifierQueue(
    appType: string,
    providerId: string,
  ): Promise<void> {
    return invoke("add_to_classifier_queue", { appType, providerId });
  },

  // 从分类器队列移除供应商
  async removeFromClassifierQueue(
    appType: string,
    providerId: string,
  ): Promise<void> {
    return invoke("remove_from_classifier_queue", { appType, providerId });
  },

  // 读取分类器队列的两个开关
  async getClassifierConfig(appType: string): Promise<ClassifierConfig> {
    return invoke("get_classifier_config", { appType });
  },

  // 写入分类器队列的两个开关
  async setClassifierConfig(
    appType: string,
    config: ClassifierConfig,
  ): Promise<void> {
    return invoke("set_classifier_config", { appType, config });
  },
};
