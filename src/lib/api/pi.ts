import { invoke } from "@tauri-apps/api/core";

export interface PiProviderConfig {
  baseUrl: string;
  api: string;
  apiKey: string;
  authHeader?: boolean;
  headers?: Record<string, unknown>;
  compat?: {
    supportsDeveloperRole?: boolean;
    supportsReasoningEffort?: boolean;
  };
  models: PiModelConfig[];
}

export interface PiModelConfig {
  id: string;
  name: string;
  reasoning?: boolean;
  input?: string[];
  contextWindow: number;
  maxTokens: number;
  cost: {
    input: number;
    output: number;
    cacheRead: number;
    cacheWrite: number;
  };
}

export interface PiSettings {
  defaultProvider?: string;
  defaultModel?: string;
  defaultThinkingLevel?: string;
  hideThinkingBlock?: boolean;
  theme?: string;
  quietStartup?: boolean;
  compaction?: { enabled?: boolean };
  retry?: { enabled?: boolean; maxRetries?: number };
}

// Provider CRUD
export async function getPiProviders(): Promise<Record<string, PiProviderConfig>> {
  return invoke("get_pi_providers");
}

export async function setPiProvider(
  providerId: string,
  config: PiProviderConfig,
): Promise<void> {
  return invoke("set_pi_provider", { providerId, config });
}

export async function removePiProvider(providerId: string): Promise<void> {
  return invoke("remove_pi_provider", { providerId });
}

export async function setActivePiProvider(
  providerId: string,
  modelId?: string,
): Promise<void> {
  return invoke("set_active_pi_provider", { providerId, modelId });
}

// Settings
export async function getPiSettings(): Promise<PiSettings> {
  return invoke("get_pi_settings");
}

export async function updatePiSettings(
  fields: Record<string, unknown>,
): Promise<void> {
  return invoke("update_pi_settings", { fields });
}

// Import from live
export async function importPiProvidersFromLive(): Promise<number> {
  return invoke("import_pi_providers_from_live");
}
