import { invoke } from "@tauri-apps/api/core";
import type {
  ProxyStatus,
  ProxyServerInfo,
  ProxyTakeoverStatus,
  GlobalProxyConfig,
  AppProxyConfig,
  PublicRouteStatus,
  NamedTunnel,
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

  // 为指定应用开启/关闭接管
  async setProxyTakeoverForApp(
    appType: string,
    enabled: boolean,
  ): Promise<void> {
    return invoke("set_proxy_takeover_for_app", { appType, enabled });
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

  // ========== 公网路由 API ==========

  // 查询公网路由状态
  async getPublicRouteStatus(): Promise<PublicRouteStatus> {
    return invoke("get_public_route_status");
  },

  // 启用公网路由（启动 cloudflared 隧道）
  async enablePublicRoute(): Promise<PublicRouteStatus> {
    return invoke("enable_public_route");
  },

  // 禁用公网路由（停止隧道）
  async disablePublicRoute(): Promise<PublicRouteStatus> {
    return invoke("disable_public_route");
  },

  // 重新生成公网路由隧道鉴权密钥（ccsk-*）；Cursor 的 key 无法自动写入，需用户重新粘贴
  async regeneratePublicRouteApiKey(): Promise<PublicRouteStatus> {
    return invoke("regenerate_public_route_api_key");
  },

  // 保存公网路由隧道配置（模式 + 命名隧道参数）；开启中会立即重建隧道
  async setPublicRouteTunnelConfig(config: {
    mode: string;
    namedTunnel?: string | null;
    namedHostname?: string | null;
  }): Promise<PublicRouteStatus> {
    return invoke("set_public_route_tunnel_config", config);
  },

  // 列出 Cloudflare 账户下已有的命名隧道（需已 cloudflared login）
  async listNamedTunnels(): Promise<NamedTunnel[]> {
    return invoke("list_named_tunnels");
  },

  // ========== 计费默认配置 API ==========

  // 获取默认成本倍率
  async getDefaultCostMultiplier(appType: string): Promise<string> {
    return invoke("get_default_cost_multiplier", { appType });
  },

  // 设置默认成本倍率
  async setDefaultCostMultiplier(
    appType: string,
    value: string,
  ): Promise<void> {
    return invoke("set_default_cost_multiplier", { appType, value });
  },

  // 获取计费模式来源
  async getPricingModelSource(appType: string): Promise<string> {
    return invoke("get_pricing_model_source", { appType });
  },

  // 设置计费模式来源
  async setPricingModelSource(appType: string, value: string): Promise<void> {
    return invoke("set_pricing_model_source", { appType, value });
  },
};
