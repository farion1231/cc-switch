import { DEEPSEEK_HARNESS_DEFAULT_CONFIG } from "@/config/deepseekHarnessProviderPresets";

// ── Default configs ──────────────────────────────────────────────────

export const DSH_MODEL_API_OPTIONS = [
  "openai-completions",
  "openai-responses",
  "anthropic-messages",
] as const;

export type DshModelApi = (typeof DSH_MODEL_API_OPTIONS)[number];

export type DshModel = {
  id: string;
  name?: string;
  contextWindow?: number;
};

export const DSH_OFFICIAL_DEFAULT_CONFIG = DEEPSEEK_HARNESS_DEFAULT_CONFIG;

export const DSH_CUSTOM_DEFAULT_CONFIG: Record<string, unknown> = {
  displayName: "Custom DSH",
  api: "openai-completions",
  baseURL: "",
  apiKeyEnv: "CUSTOM_DSH_API_KEY",
  apiKey: "",
  models: [],
};

export interface DshSettingsConfigInput {
  apiKey: string;
  baseURL: string;
  api: string;
  apiKeyEnv: string;
  models: DshModel[];
  displayName?: string;
}

// ── Pure functions ───────────────────────────────────────────────────

/**
 * Official DSH routes are identified by the fixed provider id, the official
 * category, or (as a last resort) the reserved credential reference.
 */
export function isDshOfficial(
  id?: string,
  category?: string,
  config?: Record<string, unknown> | null,
): boolean {
  if (id === "deepseek-official") return true;
  if (category === "official") return true;
  if (config && config.apiKeyEnv === "DEEPSEEK_API_KEY") return true;
  return false;
}

/**
 * Safely parses a DSH model catalog, tolerating `{ id }` objects and bare
 * id strings from imported native configuration.
 */
export function normalizeDshModels(value: unknown): DshModel[] {
  if (!Array.isArray(value)) return [];

  const models: DshModel[] = [];
  for (const entry of value) {
    if (typeof entry === "string") {
      models.push({ id: entry });
      continue;
    }
    if (entry && typeof entry === "object" && !Array.isArray(entry)) {
      const item = entry as Record<string, unknown>;
      const id = item.id == null ? "" : String(item.id);
      const name =
        typeof item.name === "string" && item.name ? item.name : undefined;
      const contextWindow =
        typeof item.contextWindow === "number" &&
        Number.isFinite(item.contextWindow)
          ? item.contextWindow
          : undefined;
      models.push({ id, name, contextWindow });
    }
  }
  return models;
}

/** DSH credential reference names must match the backend's env-var pattern. */
export function isValidCredentialRef(value: string): boolean {
  return /^[A-Z_][A-Z0-9_]*$/.test(value);
}

/**
 * Assembles the persisted `settings_config` for the current state, keeping the
 * official and custom (pi-ai) shapes distinct.
 */
export function buildDshSettingsConfig(
  state: DshSettingsConfigInput,
  isOfficial: boolean,
): Record<string, unknown> {
  const models = (state.models || [])
    .map((model) => {
      const id = model.id.trim();
      const name = model.name?.trim() || id;
      return {
        id,
        name,
        ...(model.contextWindow ? { contextWindow: model.contextWindow } : {}),
      };
    })
    .filter((model) => model.id);

  if (isOfficial) {
    return {
      apiKey: state.apiKey.trim(),
      baseURL: state.baseURL.trim(),
      profile: "desktop",
      models,
    };
  }

  return {
    displayName: (state.displayName ?? "").trim(),
    api: state.api,
    baseURL: state.baseURL.trim(),
    apiKeyEnv: state.apiKeyEnv.trim(),
    apiKey: state.apiKey.trim(),
    models,
  };
}
