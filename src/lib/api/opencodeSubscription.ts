import { invoke } from "@tauri-apps/api/core";

export type OpenCodeSubscriptionKind = "go" | "zen";

export interface SaveOpenCodeSubscriptionProviderRequest {
  providerId?: string | null;
  name?: string | null;
  subscriptionKind: OpenCodeSubscriptionKind;
  baseUrl: string;
  apiKey: string;
  defaultModel?: string | null;
}

export interface OpenCodeSubscriptionProviderRecord {
  id: string;
  providerId: string;
  subscriptionKind: OpenCodeSubscriptionKind;
  baseUrl: string;
  apiKeyRef: string;
  localAdapterEnabled: boolean;
  defaultModel?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface OpenCodeSubscriptionConnectionResult {
  success: boolean;
  providerId: string;
  status?: number | null;
  latencyMs: number;
  message: string;
  models: string[];
}

export interface OpenCodeSubscriptionStreamResult {
  success: boolean;
  providerId: string;
  status?: number | null;
  latencyMs: number;
  firstEvent?: string | null;
  message: string;
}

export interface OpenCodeSubscriptionError {
  code: string;
  message: string;
  suggestion: string;
  details?: string | null;
}

export const opencodeSubscriptionApi = {
  async saveProvider(
    req: SaveOpenCodeSubscriptionProviderRequest,
  ): Promise<OpenCodeSubscriptionProviderRecord> {
    return await invoke("opencode_subscription_save_provider", { req });
  },

  async testConnection(
    providerId: string,
  ): Promise<OpenCodeSubscriptionConnectionResult> {
    return await invoke("opencode_subscription_test_connection", {
      providerId,
    });
  },

  async testStream(
    providerId: string,
  ): Promise<OpenCodeSubscriptionStreamResult> {
    return await invoke("opencode_subscription_test_stream", { providerId });
  },

  async listModels(providerId: string): Promise<string[]> {
    return await invoke("opencode_subscription_list_models", { providerId });
  },
};
