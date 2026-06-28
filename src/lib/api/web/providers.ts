import { get, post, put, del, connectWebSocket } from "../web-client";
import type {
  Provider,
  UniversalProvider,
  UniversalProvidersMap,
} from "@/types";
import type { AppId } from "./types";
import type { ClaudeDesktopStatus, SwitchResult } from "../providers";
import type { ClaudeDesktopDefaultRoute } from "../providers";

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
    return get(`/providers?app=${appId}`);
  },

  async getCurrent(appId: AppId): Promise<string | null> {
    return get(`/providers/current?app=${appId}`);
  },

  async add(provider: Provider, appId: AppId): Promise<boolean> {
    return post("/providers", { provider, app: appId });
  },

  async update(
    provider: Provider,
    appId: AppId,
    originalId?: string,
  ): Promise<boolean> {
    return put(`/providers/${originalId ?? provider.id}`, {
      provider,
      app: appId,
      originalId,
    });
  },

  async delete(id: string, appId: AppId): Promise<boolean> {
    return del(`/providers/${id}?app=${appId}`);
  },

  async removeFromLiveConfig(id: string, appId: AppId): Promise<boolean> {
    return post(`/providers/${id}/remove-from-live`, { app: appId });
  },

  async switch(id: string, appId: AppId): Promise<SwitchResult> {
    await post(`/providers/${id}/switch?app=${encodeURIComponent(appId)}`, {});
    return { warnings: [] };
  },

  async importDefault(appId: AppId): Promise<boolean> {
    return post("/providers/import-default", { app: appId });
  },

  async updateTrayMenu(): Promise<boolean> {
    console.warn("update_tray_menu not available in web mode");
    return true;
  },

  async updateSortOrder(
    updates: ProviderSortUpdate[],
    appId: AppId,
  ): Promise<boolean> {
    return post("/providers/sort", { updates, app: appId });
  },

  onSwitched(handler: (event: ProviderSwitchEvent) => void): () => void {
    return connectWebSocket((data: any) => {
      if (data.event === "provider.switched") {
        handler({
          appType: data.data.app,
          providerId: data.data.id,
        });
      }
    });
  },

  async openTerminal(
    _providerId: string,
    _appId: AppId,
    _options?: { cwd?: string },
  ): Promise<boolean> {
    console.warn("open_provider_terminal not available in web mode");
    return false;
  },

  async importOpenCodeFromLive(): Promise<number> {
    return post("/providers/import-opencode-live", {});
  },

  async getOpenCodeLiveProviderIds(): Promise<string[]> {
    return get("/providers/opencode-live-ids");
  },

  async getOpenClawLiveProviderIds(): Promise<string[]> {
    return get("/providers/openclaw-live-ids");
  },

  async getHermesLiveProviderIds(): Promise<string[]> {
    return get("/providers/hermes-live-ids");
  },

  async getClaudeDesktopStatus(): Promise<ClaudeDesktopStatus> {
    return get("/providers/claude-desktop-status");
  },

  async getClaudeDesktopDefaultRoutes(): Promise<ClaudeDesktopDefaultRoute[]> {
    return get("/providers/claude-desktop-default-routes");
  },

  async importOpenClawFromLive(): Promise<number> {
    return post("/providers/import-openclaw-live", {});
  },

  async importHermesFromLive(): Promise<number> {
    return post("/providers/import-hermes-live", {});
  },

  async importClaudeDesktopFromClaude(): Promise<number> {
    return post("/providers/import-claude-desktop-from-claude", {});
  },

  async ensureClaudeDesktopOfficialProvider(): Promise<boolean> {
    return post("/providers/ensure-claude-desktop-official", {});
  },
};

export const universalProvidersApi = {
  async getAll(): Promise<UniversalProvidersMap> {
    return get("/universal-providers");
  },

  async get(id: string): Promise<UniversalProvider | null> {
    return get(`/universal-providers/${id}`);
  },

  async upsert(provider: UniversalProvider): Promise<boolean> {
    return post("/universal-providers", provider);
  },

  async delete(id: string): Promise<boolean> {
    return del(`/universal-providers/${id}`);
  },

  async sync(id: string): Promise<boolean> {
    return post(`/universal-providers/${id}/sync`, {});
  },
};
