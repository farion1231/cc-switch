import { invoke } from "@tauri-apps/api/core";
import type { Provider } from "@/types";

export type CursorProviderType = "openai" | "anthropic";

export interface CursorModelConfig {
  enabled: boolean;
  type: CursorProviderType;
  providerGroup: string;
  endpointId: string;
  baseURL: string;
  apiKey: string;
  modelID: string;
  pricingModel: string;
  tooltipData: string;
  reasoningEffort: string;
  openAIEndpoint: string;
  openAIExtraParamsEnabled: boolean;
  openAIExtraParamsJSON: string;
  customHeadersEnabled: boolean;
  customHeadersJSON: string;
  anthropicExtraParamsEnabled: boolean;
  anthropicExtraParamsJSON: string;
  contextWindowTokens: number;
  maxCompletionTokens: number;
  anthropicMaxTokens: number;
  anthropicThinkingEffort: string;
  thinkingBudgetTokens: number;
}

export interface CursorProvider extends Provider {
  settingsConfig: CursorModelConfig;
}

export interface CursorEndpoint {
  id: string;
  name: string;
  type: CursorProviderType;
  baseURL: string;
  apiKey: string;
  createdAt: number;
}

export interface CursorProviderChanges {
  endpoint: CursorEndpoint;
  upserts: CursorProvider[];
  deletedProviderIds: string[];
}

export const createCursorProviderChanges = (
  endpoint: CursorEndpoint,
  originalProviders: CursorProvider[],
  upserts: CursorProvider[],
): CursorProviderChanges => {
  const retainedProviderIds = new Set(upserts.map(({ id }) => id));
  return {
    endpoint,
    upserts,
    deletedProviderIds: originalProviders
      .map(({ id }) => id)
      .filter((id) => !retainedProviderIds.has(id)),
  };
};

export const groupCursorProvidersByEndpoint = (
  endpoints: CursorEndpoint[],
  providers: CursorProvider[],
) =>
  endpoints.map((endpoint) => ({
    endpoint,
    providers: providers.filter(
      (provider) => provider.settingsConfig.endpointId === endpoint.id,
    ),
  }));

export type CursorRuntimePhase =
  | "stopped"
  | "starting"
  | "running"
  | "restoring"
  | "testing"
  | "maintenance"
  | "error";

export interface CursorRuntimeState {
  phase: CursorRuntimePhase;
  sidecarRunning: boolean;
  backendListenAddr: string;
  backendRunning: boolean;
  proxyListenAddr: string;
  proxyRunning: boolean;
  cursorSettingsApplied: boolean;
  caInstalled: boolean;
  caFingerprint: string;
  platform: string;
  lastError: string;
}

export interface CursorModelTestResult {
  adapterId: string;
  status: "running" | "success" | "error";
  tokensPerSecond: number;
  firstTextTokenMs: number;
  totalDurationMs: number;
  outputTokens: number;
  error?: string;
}

export const createCursorModelConfig = (
  overrides: Partial<CursorModelConfig> = {},
): CursorModelConfig => ({
  enabled: true,
  type: "openai",
  providerGroup: "",
  endpointId: "",
  baseURL: "https://api.openai.com",
  apiKey: "",
  modelID: "",
  pricingModel: "",
  tooltipData: "Managed by CC Switch",
  reasoningEffort: "medium",
  openAIEndpoint: "/v1/responses",
  openAIExtraParamsEnabled: false,
  openAIExtraParamsJSON: "{}",
  customHeadersEnabled: false,
  customHeadersJSON: "{}",
  anthropicExtraParamsEnabled: false,
  anthropicExtraParamsJSON: "{}",
  contextWindowTokens: 0,
  maxCompletionTokens: 0,
  anthropicMaxTokens: 0,
  anthropicThinkingEffort: "xhigh",
  thinkingBudgetTokens: 0,
  ...overrides,
});

export const normalizeCursorProviders = (
  providers: Record<string, Provider>,
): Record<string, CursorProvider> =>
  Object.fromEntries(
    Object.entries(providers).map(([id, provider]) => [
      id,
      {
        ...provider,
        settingsConfig: createCursorModelConfig(provider.settingsConfig),
      } satisfies CursorProvider,
    ]),
  );

export const cursorApi = {
  getEndpoints: () => invoke<CursorEndpoint[]>("get_cursor_endpoints"),
  getProviders: async (): Promise<Record<string, CursorProvider>> =>
    normalizeCursorProviders(
      await invoke<Record<string, Provider>>("get_cursor_providers"),
    ),
  saveProvider: (provider: CursorProvider) =>
    invoke<boolean>("save_cursor_provider", { provider }),
  saveProviders: (changes: CursorProviderChanges) =>
    invoke<boolean>("save_cursor_providers", { changes }),
  deleteEndpoint: (id: string) =>
    invoke<boolean>("delete_cursor_endpoint", { id }),
  deleteProvider: (id: string) =>
    invoke<boolean>("delete_cursor_provider", { id }),
  setProviderEnabled: (id: string, enabled: boolean) =>
    invoke<boolean>("set_cursor_provider_enabled", { id, enabled }),
  getRuntimeState: () => invoke<CursorRuntimeState>("get_cursor_runtime_state"),
  startRuntime: () => invoke<CursorRuntimeState>("start_cursor_runtime"),
  stopRuntime: () => invoke<CursorRuntimeState>("stop_cursor_runtime"),
  installCA: () => invoke<CursorRuntimeState>("install_cursor_ca"),
  removeCA: () => invoke<CursorRuntimeState>("remove_cursor_ca"),
  syncUsage: () => invoke<number>("sync_cursor_usage"),
  testModel: (providerId: string) =>
    invoke<CursorModelTestResult>("test_cursor_model", { providerId }),
};
