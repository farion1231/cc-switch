import { invoke } from "@tauri-apps/api/core";
import type {
  ProxyConfig,
  ProxyStatus,
  ProxyServerInfo,
  ProxyTakeoverStatus,
  GlobalProxyConfig,
  AppProxyConfig,
} from "@/types/proxy";

export const proxyApi = {
  // ========== 代理服务器控制 API ==========

  // 启动代理服务器
  async startProxyServer(): Promise<ProxyServerInfo> {
    return invoke("start_proxy_server");
  },

  // 停止代理服务器并恢复配置
  async stopProxyWithRestore(): Promise<void> {
    return invoke("stop_proxy_with_restore");
  },

  // 获取代理服务器状态
  async getProxyStatus(): Promise<ProxyStatus> {
    return invoke("get_proxy_status");
  },

  // 检查代理服务器是否正在运行
  async isProxyRunning(): Promise<boolean> {
    return invoke("is_proxy_running");
  },

  // 检查是否处于接管模式
  async isLiveTakeoverActive(): Promise<boolean> {
    return invoke("is_live_takeover_active");
  },

  // 代理模式下切换供应商
  async switchProxyProvider(
    appType: string,
    providerId: string,
  ): Promise<void> {
    return invoke("switch_proxy_provider", { appType, providerId });
  },

  // ========== 接管状态 API ==========

  // 获取各应用接管状态
  async getProxyTakeoverStatus(): Promise<ProxyTakeoverStatus> {
    return invoke("get_proxy_takeover_status");
  },

  // 为指定应用开启/关闭接管
  async setProxyTakeoverForApp(
    appType: string,
    enabled: boolean,
  ): Promise<void> {
    return invoke("set_proxy_takeover_for_app", { appType, enabled });
  },

  // ========== Legacy 代理配置 API (兼容) ==========

  // 获取代理配置（旧版 v2 兼容接口）
  async getProxyConfig(): Promise<ProxyConfig> {
    return invoke("get_proxy_config");
  },

  // 更新代理配置（旧版 v2 兼容接口）
  async updateProxyConfig(config: ProxyConfig): Promise<void> {
    return invoke("update_proxy_config", { config });
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
};
