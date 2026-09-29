import { invoke } from "@tauri-apps/api/core";
import type {
  ProxyStatus,
  ProxyServerInfo,
  ProxyTakeoverStatus,
  GlobalProxyConfig,
  AppProxyConfig,
  ProxyPool,
  ProxyPoolNotice,
} from "@/types/proxy";

export const proxyApi = {
  // ========== 代理服务器控制 API ==========

  // 启动代理服务器
  async startProxyServer(): Promise<ProxyServerInfo> {
    return invoke("start_proxy_server");
  },

  // 停止代理服务器（不恢复已接管配置）
  async stopProxyServer(): Promise<void> {
    return invoke("stop_proxy_server");
  },

  // 停止代理服务器并恢复配置
  async stopProxyWithRestore(): Promise<void> {
    return invoke("stop_proxy_with_restore");
  },

  // 获取代理服务器状态
  async getProxyStatus(): Promise<ProxyStatus> {
    return invoke("get_proxy_status");
  },

  // ========== 接管状态 API ==========

  // 获取各应用接管状态
  async getProxyTakeoverStatus(): Promise<ProxyTakeoverStatus> {
    return invoke("get_proxy_takeover_status");
  },

  // 为指定应用开启/关闭接管。pool 为真时进入的是附加模式（和路由模式二选一）
  async setProxyTakeoverForApp(
    appType: string,
    enabled: boolean,
    pool = false,
  ): Promise<void> {
    return invoke("set_proxy_takeover_for_app", { appType, enabled, pool });
  },

  // 设置里在路由和附加之间换时：处于另一种模式（pool 为真是附加模式）的 Claude Code、
  // Codex 先退回直连。返回退回直连的应用
  async exitProxyAppsInMode(pool: boolean): Promise<string[]> {
    return invoke("exit_proxy_apps_in_mode", { pool });
  },

  // 直连供应商：路由模式下退出路由时写回的那家（和路由到的那家互相独立）
  async getDirectProvider(appType: string): Promise<string | null> {
    return invoke("get_direct_provider", { appType });
  },

  // ========== 附加模型 API ==========

  // 附加模型名单：每一家和它发布的模型 id，以及提示
  async getProxyPool(appType: string): Promise<ProxyPool> {
    return invoke("get_proxy_pool", { appType });
  },

  // 把一家加入或移出附加模型（enabled 是目标值）。成功时返回客户端看不到或看不全附加模型
  // 的提示；失败时抛出 ProxyPoolWriteError
  async setProxyPoolMember(
    appType: string,
    providerId: string,
    enabled: boolean,
  ): Promise<ProxyPoolNotice | null> {
    return invoke("set_proxy_pool_member", { appType, providerId, enabled });
  },

  // ========== v3+ 全局/应用级配置 API ==========

  // 获取全局代理配置
  async getGlobalProxyConfig(): Promise<GlobalProxyConfig> {
    return invoke("get_global_proxy_config");
  },

  // 更新全局代理配置
  async updateGlobalProxyConfig(config: GlobalProxyConfig): Promise<void> {
    return invoke("update_global_proxy_config", { config });
  },

  // 获取指定应用的代理配置
  async getProxyConfigForApp(appType: string): Promise<AppProxyConfig> {
    return invoke("get_proxy_config_for_app", { appType });
  },

  // 更新指定应用的代理配置
  async updateProxyConfigForApp(config: AppProxyConfig): Promise<void> {
    return invoke("update_proxy_config_for_app", { config });
  },

  // ========== 计费默认配置 API ==========

  // 获取计费模式来源
  async getPricingModelSource(appType: string): Promise<string> {
    return invoke("get_pricing_model_source", { appType });
  },

  // 设置计费模式来源
  async setPricingModelSource(appType: string, value: string): Promise<void> {
    return invoke("set_pricing_model_source", { appType, value });
  },
};
