import { get, post, put } from "../web-client";
import type {
  ProxyConfig,
  ProxyStatus,
  ProxyServerInfo,
  ProxyTakeoverStatus,
  GlobalProxyConfig,
  AppProxyConfig,
} from "@/types/proxy";

export const proxyApi = {
  async startProxyServer(): Promise<ProxyServerInfo> {
    return post("/proxy/start", {});
  },

  async stopProxyServer(): Promise<void> {
    await post("/proxy/stop", {});
  },

  async stopProxyWithRestore(): Promise<void> {
    await post("/proxy/stop-with-restore", {});
  },

  async getProxyStatus(): Promise<ProxyStatus> {
    return get("/proxy/status");
  },

  async isProxyRunning(): Promise<boolean> {
    const status = await get<ProxyStatus>("/proxy/status");
    return status.running;
  },

  async isLiveTakeoverActive(): Promise<boolean> {
    const status = await get<ProxyTakeoverStatus>("/proxy/takeover");
    return Object.values(status).some((v) => v === true);
  },

  async switchProxyProvider(
    appType: string,
    providerId: string,
  ): Promise<void> {
    await post("/proxy/switch", { appType, providerId });
  },

  async getProxyTakeoverStatus(): Promise<ProxyTakeoverStatus> {
    return get("/proxy/takeover");
  },

  async setProxyTakeoverForApp(
    appType: string,
    enabled: boolean,
  ): Promise<void> {
    await post("/proxy/takeover", { app: appType, enabled });
  },

  async getProxyConfig(): Promise<ProxyConfig> {
    return get("/proxy/config");
  },

  async updateProxyConfig(config: ProxyConfig): Promise<void> {
    await post("/proxy/config", config);
  },

  async getGlobalProxyConfig(): Promise<GlobalProxyConfig> {
    return get("/proxy/config/global");
  },

  async updateGlobalProxyConfig(config: GlobalProxyConfig): Promise<void> {
    await post("/proxy/config/global", config);
  },

  async getProxyConfigForApp(appType: string): Promise<AppProxyConfig> {
    return get(`/proxy/config/app?app=${appType}`);
  },

  async updateProxyConfigForApp(config: AppProxyConfig): Promise<void> {
    await post(`/proxy/config/app?app=${config.appType}`, config);
  },

  async getDefaultCostMultiplier(appType: string): Promise<string> {
    return get(
      `/proxy/default-cost-multiplier?app=${encodeURIComponent(appType)}`,
    );
  },

  async setDefaultCostMultiplier(
    appType: string,
    value: string,
  ): Promise<void> {
    await put(
      `/proxy/default-cost-multiplier?app=${encodeURIComponent(appType)}`,
      {
        value,
      },
    );
  },

  async getPricingModelSource(appType: string): Promise<string> {
    return get(
      `/proxy/pricing-model-source?app=${encodeURIComponent(appType)}`,
    );
  },

  async setPricingModelSource(appType: string, value: string): Promise<void> {
    await put(
      `/proxy/pricing-model-source?app=${encodeURIComponent(appType)}`,
      {
        value,
      },
    );
  },
};
