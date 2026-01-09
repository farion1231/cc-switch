import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Provider,
  UniversalProvider,
  UniversalProvidersMap,
} from "@/types";
import type { AppId } from "./types";

export interface ProviderSortUpdate {
  id: string;
  sortIndex: number;
}

export interface ProviderSwitchEvent {
  appType: AppId;
  providerId: string;
}

export const providersApi = {
  async getAll(appId: AppId): Promise<Record<string, Provider>> {
    return await invoke("get_providers", { app: appId });
  },

  async getCurrent(appId: AppId): Promise<string> {
    return await invoke("get_current_provider", { app: appId });
  },

  async add(provider: Provider, appId: AppId): Promise<boolean> {
    return await invoke("add_provider", { provider, app: appId });
  },

  async update(provider: Provider, appId: AppId): Promise<boolean> {
    return await invoke("update_provider", { provider, app: appId });
  },

  async delete(id: string, appId: AppId): Promise<boolean> {
    return await invoke("delete_provider", { id, app: appId });
  },

  async switch(id: string, appId: AppId): Promise<boolean> {
    return await invoke("switch_provider", { id, app: appId });
  },

  async importDefault(appId: AppId): Promise<boolean> {
    return await invoke("import_default_config", { app: appId });
  },

  async updateTrayMenu(): Promise<boolean> {
    return await invoke("update_tray_menu");
  },

  async updateSortOrder(
    updates: ProviderSortUpdate[],
    appId: AppId,
  ): Promise<boolean> {
    return await invoke("update_providers_sort_order", { updates, app: appId });
  },

  async onSwitched(
    handler: (event: ProviderSwitchEvent) => void,
  ): Promise<UnlistenFn> {
    return await listen("provider-switched", (event) => {
      const payload = event.payload as ProviderSwitchEvent;
      handler(payload);
    });
  },

  /**
   * 打开指定提供商的终端
   * 任何提供商都可以打开终端，不受是否为当前激活提供商的限制
   * 终端会使用该提供商特定的 API 配置，不影响全局设置
   */
  async openTerminal(providerId: string, appId: AppId): Promise<boolean> {
    return await invoke("open_provider_terminal", { providerId, app: appId });
  },
};

// ============================================================================
// 统一供应商（Universal Provider）API
// ============================================================================

export const universalProvidersApi = {
  /**
   * 获取所有统一供应商
   */
  async getAll(): Promise<UniversalProvidersMap> {
    return await invoke("get_universal_providers");
  },

  /**
   * 获取单个统一供应商
   */
  async get(id: string): Promise<UniversalProvider | null> {
    return await invoke("get_universal_provider", { id });
  },

  /**
   * 添加或更新统一供应商
   */
  async upsert(provider: UniversalProvider): Promise<boolean> {
    return await invoke("upsert_universal_provider", { provider });
  },

  /**
   * 删除统一供应商
   */
  async delete(id: string): Promise<boolean> {
    return await invoke("delete_universal_provider", { id });
  },

  /**
   * 手动同步统一供应商到各应用
   */
  async sync(id: string): Promise<boolean> {
    return await invoke("sync_universal_provider", { id });
  },
};
