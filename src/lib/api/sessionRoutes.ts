import { invoke } from "@tauri-apps/api/core";

export interface SessionRouteInfo {
  sessionId: string;
  appType: string;
  providerId: string;
  providerName: string;
  assignedAt: number;
  lastUsedAt: number;
  requestCount: number;
  failoverCount: number;
}

export interface SessionRoutingConfig {
  enabled: boolean;
  strategy: "round_robin" | "least_loaded";
  sessionTtlSeconds: number;
  maxSessionsPerProvider: number;
}

export interface ProviderLoadInfo {
  providerId: string;
  providerName: string;
  sessionCount: number;
}

export const sessionRoutesApi = {
  async getConfig(appType: string): Promise<SessionRoutingConfig> {
    return invoke("get_session_routing_config", { appType });
  },

  async updateConfig(
    appType: string,
    config: SessionRoutingConfig,
  ): Promise<void> {
    return invoke("update_session_routing_config", { appType, config });
  },

  async getActiveRoutes(appType: string): Promise<SessionRouteInfo[]> {
    return invoke("get_active_session_routes", { appType });
  },

  async deleteRoute(sessionId: string, appType: string): Promise<void> {
    return invoke("delete_session_route", { sessionId, appType });
  },

  async setRouteProvider(
    sessionId: string,
    appType: string,
    providerId: string,
  ): Promise<void> {
    return invoke("set_session_route_provider", {
      sessionId,
      appType,
      providerId,
    });
  },

  async cleanupExpired(
    appType: string,
    ttlSeconds: number,
  ): Promise<number> {
    return invoke("cleanup_expired_session_routes", { appType, ttlSeconds });
  },

  async getProviderLoad(
    appType: string,
  ): Promise<ProviderLoadInfo[]> {
    return invoke("get_session_provider_load", { appType });
  },
};