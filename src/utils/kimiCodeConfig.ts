/**
 * Kimi Code provider-owned TOML fragment helpers (frontend).
 *
 * Fragment shape (scoped, never includes shared top-level live fields):
 *
 * selected_model = "alias"
 *
 * [providers.<id>]
 * type = "openai"
 * base_url = "..."
 * api_key = "..."
 *
 * [models."<alias>"]
 * provider = "<id>"
 * model = "..."
 * max_context_size = 128000
 */

import { parse as parseToml } from "smol-toml";

export const KIMI_PROVIDER_TYPES = [
  "kimi",
  "anthropic",
  "openai",
  "openai_responses",
  "google-genai",
  "vertexai",
] as const;

export type KimiProviderType = (typeof KIMI_PROVIDER_TYPES)[number];

export interface KimiModelEntry {
  alias: string;
  model: string;
  maxContextSize: number;
  maxOutputSize?: number;
  capabilities?: string[];
  displayName?: string;
  reasoningKey?: string;
}

export interface KimiProviderFormState {
  providerId: string;
  providerType: KimiProviderType;
  baseUrl: string;
  apiKey: string;
  customHeaders: Record<string, string>;
  models: KimiModelEntry[];
  selectedModel: string;
}

const DEFAULT_STATE: KimiProviderFormState = {
  providerId: "custom",
  providerType: "openai",
  baseUrl: "",
  apiKey: "",
  customHeaders: {},
  models: [
    {
      alias: "custom/default",
      model: "default",
      maxContextSize: 128000,
    },
  ],
  selectedModel: "custom/default",
};

function quoteTomlKey(key: string): string {
  if (/^[A-Za-z0-9_-]+$/.test(key)) return key;
  return `"${key.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

function quoteTomlString(value: string): string {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

/** Build provider-owned TOML fragment from form state. */
export function buildKimiCodeConfig(state: KimiProviderFormState): string {
  const lines: string[] = [];
  const selected =
    state.selectedModel.trim() || state.models[0]?.alias || "default";
  lines.push(`selected_model = ${quoteTomlString(selected)}`);
  lines.push("");
  lines.push(`[providers.${quoteTomlKey(state.providerId)}]`);
  lines.push(`type = ${quoteTomlString(state.providerType)}`);
  if (state.baseUrl.trim()) {
    lines.push(`base_url = ${quoteTomlString(state.baseUrl.trim())}`);
  }
  if (state.apiKey.trim()) {
    lines.push(`api_key = ${quoteTomlString(state.apiKey.trim())}`);
  }
  const headerKeys = Object.keys(state.customHeaders).filter(
    (k) => k.trim() && state.customHeaders[k]?.trim(),
  );
  if (headerKeys.length > 0) {
    lines.push("");
    lines.push(`[providers.${quoteTomlKey(state.providerId)}.custom_headers]`);
    for (const key of headerKeys) {
      lines.push(
        `${quoteTomlKey(key.trim())} = ${quoteTomlString(state.customHeaders[key].trim())}`,
      );
    }
  }

  for (const model of state.models) {
    if (!model.alias.trim()) continue;
    lines.push("");
    lines.push(`[models.${quoteTomlKey(model.alias.trim())}]`);
    lines.push(`provider = ${quoteTomlString(state.providerId)}`);
    lines.push(`model = ${quoteTomlString(model.model.trim() || model.alias)}`);
    lines.push(
      `max_context_size = ${Math.max(1, Math.floor(model.maxContextSize || 1))}`,
    );
    if (model.maxOutputSize && model.maxOutputSize > 0) {
      lines.push(`max_output_size = ${Math.floor(model.maxOutputSize)}`);
    }
    if (model.capabilities && model.capabilities.length > 0) {
      const arr = model.capabilities.map((c) => quoteTomlString(c)).join(", ");
      lines.push(`capabilities = [ ${arr} ]`);
    }
    if (model.displayName?.trim()) {
      lines.push(`display_name = ${quoteTomlString(model.displayName.trim())}`);
    }
    if (model.reasoningKey?.trim()) {
      lines.push(
        `reasoning_key = ${quoteTomlString(model.reasoningKey.trim())}`,
      );
    }
  }

  return lines.join("\n") + "\n";
}

/** Best-effort parse of a scoped fragment for form editing. */
export function parseKimiCodeConfig(
  configToml?: string,
  fallbackName?: string,
): KimiProviderFormState {
  if (!configToml || !configToml.trim()) {
    const id =
      (fallbackName || "custom")
        .toLowerCase()
        .replace(/[^a-z0-9_-]+/g, "-")
        .replace(/^-+|-+$/g, "") || "custom";
    return {
      ...DEFAULT_STATE,
      providerId: id,
      models: [
        {
          alias: `${id}/default`,
          model: "default",
          maxContextSize: 128000,
        },
      ],
      selectedModel: `${id}/default`,
    };
  }

  let parsed: Record<string, unknown> = {};
  try {
    parsed = parseToml(configToml) as Record<string, unknown>;
  } catch {
    return { ...DEFAULT_STATE };
  }

  const providers = (parsed.providers || {}) as Record<
    string,
    Record<string, unknown>
  >;
  const modelsTable = (parsed.models || {}) as Record<
    string,
    Record<string, unknown>
  >;
  const providerIds = Object.keys(providers);
  const providerId = providerIds[0] || "custom";
  const provider = providers[providerId] || {};

  const customHeaders: Record<string, string> = {};
  const headers = provider.custom_headers;
  if (headers && typeof headers === "object") {
    for (const [k, v] of Object.entries(headers as Record<string, unknown>)) {
      if (typeof v === "string") customHeaders[k] = v;
    }
  }

  const models: KimiModelEntry[] = Object.entries(modelsTable).map(
    ([alias, entry]) => ({
      alias,
      model: String(entry.model || alias),
      maxContextSize: Number(entry.max_context_size || 128000),
      maxOutputSize:
        entry.max_output_size != null
          ? Number(entry.max_output_size)
          : undefined,
      capabilities: Array.isArray(entry.capabilities)
        ? entry.capabilities.map(String)
        : undefined,
      displayName:
        typeof entry.display_name === "string" ? entry.display_name : undefined,
      reasoningKey:
        typeof entry.reasoning_key === "string"
          ? entry.reasoning_key
          : undefined,
    }),
  );

  const selectedModel =
    typeof parsed.selected_model === "string" && parsed.selected_model
      ? parsed.selected_model
      : models[0]?.alias || `${providerId}/default`;

  const typeRaw = String(provider.type || "openai");
  const providerType = (
    KIMI_PROVIDER_TYPES.includes(typeRaw as KimiProviderType)
      ? typeRaw
      : "openai"
  ) as KimiProviderType;

  return {
    providerId,
    providerType,
    baseUrl: typeof provider.base_url === "string" ? provider.base_url : "",
    apiKey: typeof provider.api_key === "string" ? provider.api_key : "",
    customHeaders,
    models:
      models.length > 0
        ? models
        : [
            {
              alias: `${providerId}/default`,
              model: "default",
              maxContextSize: 128000,
            },
          ],
    selectedModel,
  };
}

export function validateKimiCodeConfig(configToml: string): string | null {
  if (!configToml.trim()) return "empty";
  try {
    const parsed = parseToml(configToml) as Record<string, unknown>;
    const providers = parsed.providers as Record<string, unknown> | undefined;
    const models = parsed.models as Record<string, unknown> | undefined;
    if (!providers || Object.keys(providers).length !== 1) return "providers";
    if (!models || Object.keys(models).length < 1) return "models";
    for (const forbidden of [
      "default_model",
      "default_permission_mode",
      "thinking",
      "loop_control",
      "background",
      "image",
      "hooks",
      "services",
      "permission",
    ]) {
      if (forbidden in parsed) return "forbidden";
    }
    return null;
  } catch {
    return "toml";
  }
}

export function buildSettingsConfig(
  state: KimiProviderFormState,
  rawConfig?: string,
): Record<string, unknown> {
  const config = rawConfig?.trim() ? rawConfig : buildKimiCodeConfig(state);
  return {
    config,
    provider_id: state.providerId,
    selected_model: state.selectedModel,
  };
}
