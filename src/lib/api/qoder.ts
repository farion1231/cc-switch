import { invoke } from "@tauri-apps/api/core";
import type { ProviderMeta, UsageScript } from "@/types";

export type QoderAppId = "qoder" | "qoder-cn";
export interface QoderModel {
  model: string;
  displayName?: string;
  contextWindow?: number;
  maxOutputTokens?: number;
  capabilities?: {
    tools?: boolean;
    vision?: boolean;
    thinking?: {
      modes: string[];
      preferredMode?: string;
      [key: string]: unknown;
    };
    [key: string]: unknown;
  };
  [key: string]: unknown;
}
export interface QoderProvider {
  displayName?: string;
  type?: "openai-compatible" | "alibaba" | "aliyun-bailian";
  protocol?: "openai" | "openai-responses" | "anthropic";
  authType?: "bearer" | "api-key";
  baseUrl: string;
  apiKey: string;
  model?: string;
  models?: QoderModel[];
  [key: string]: unknown;
}
export interface QoderModels {
  providers: Record<string, QoderProvider>;
  currentModel: string;
  metadata?: Record<string, ProviderMeta>;
}

export interface QoderAuthStatus {
  installed: boolean;
  loggedIn: boolean;
  username?: string | null;
  email?: string | null;
  plan?: string | null;
  accounts?: QoderAccount[];
  currentAccountId?: string | null;
}

export interface QoderAccount {
  id: string;
  username?: string | null;
  email?: string | null;
  plan?: string | null;
  isCurrent: boolean;
}

export interface QoderLoginProgress {
  authUrl?: string | null;
  complete: boolean;
  error?: string | null;
  accountId?: string | null;
}

export const qoderKeys = {
  models: (app: QoderAppId) => ["qoder-models", app] as const,
  auth: (app: QoderAppId) => ["qoder-auth", app] as const,
  login: (app: QoderAppId, flowId: string | null) =>
    ["qoder-login", app, flowId] as const,
};
export const qoderApi = {
  getAuthStatus: (app: QoderAppId) =>
    invoke<QoderAuthStatus>("get_qoder_auth_status", { app }),
  startLogin: (app: QoderAppId, accountId?: string) =>
    invoke<string>("start_qoder_login", { app, accountId: accountId ?? null }),
  pollLogin: (app: QoderAppId, flowId: string) =>
    invoke<QoderLoginProgress>("poll_qoder_login", { app, flowId }),
  cancelLogin: (app: QoderAppId, flowId: string) =>
    invoke<void>("cancel_qoder_login", { app, flowId }),
  useAccount: (app: QoderAppId, accountId: string) =>
    invoke<QoderAuthStatus>("use_qoder_account", { app, accountId }),
  deleteAccount: (app: QoderAppId, accountId: string) =>
    invoke<QoderAuthStatus>("delete_qoder_account", { app, accountId }),
  saveUsageScript: (app: QoderAppId, id: string, script: UsageScript) =>
    invoke<QoderModels>("save_qoder_usage_script", { app, id, script }),
  getModels: (app: QoderAppId) =>
    invoke<QoderModels>("get_qoder_models", { app }),
  saveProvider: (
    app: QoderAppId,
    id: string,
    provider: QoderProvider,
    expected: QoderProvider | null,
    makeDefault: boolean,
  ) =>
    invoke<QoderModels>("save_qoder_provider", {
      app,
      id,
      provider,
      expected,
      makeDefault,
    }),
  deleteProvider: (app: QoderAppId, id: string, expected: QoderProvider) =>
    invoke<QoderModels>("delete_qoder_provider", { app, id, expected }),
  setModel: (app: QoderAppId, model: string) =>
    invoke<QoderModels>("set_qoder_model", { app, model }),
};
