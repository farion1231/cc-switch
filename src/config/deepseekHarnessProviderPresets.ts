import type { ProviderCategory } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";
import {
  opencodeProviderPresets,
  type OpenCodeProviderPreset,
} from "./opencodeProviderPresets";

export interface DeepSeekHarnessModel {
  id: string;
  name: string;
  contextWindow?: number;
}

export interface DeepSeekHarnessProviderConfig {
  // Official (`llm-deepseek`) route fields.
  apiKey?: string;
  baseURL?: string;
  profile?: string;
  // Custom (`llm-pi-ai.providers.<id>`) route fields.
  displayName?: string;
  api?: string;
  apiKeyEnv?: string;
  models?: DeepSeekHarnessModel[];
}

export interface DeepSeekHarnessProviderPreset {
  id: string;
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: DeepSeekHarnessProviderConfig;
  isOfficial?: boolean;
  category?: ProviderCategory;
  isPartner?: boolean;
  primePartner?: boolean;
  partnerPromotionKey?: string;
  templateValues?: Record<string, TemplateValueConfig>;
  theme?: PresetTheme;
  icon?: string;
  iconColor?: string;
}

export const DEEPSEEK_HARNESS_DEFAULT_CONFIG: DeepSeekHarnessProviderConfig = {
  apiKey: "",
  baseURL: "https://api.deepseek.com",
  profile: "desktop",
  models: [
    { id: "deepseek-v4-flash", name: "DeepSeek-V4-Flash" },
    { id: "deepseek-v4-pro", name: "DeepSeek-V4-Pro" },
  ],
};

const DSH_OFFICIAL_PRESET: DeepSeekHarnessProviderPreset = {
  id: "deepseek-official",
  name: "DeepSeek",
  websiteUrl: "https://platform.deepseek.com",
  apiKeyUrl: "https://platform.deepseek.com/api_keys",
  settingsConfig: DEEPSEEK_HARNESS_DEFAULT_CONFIG,
  isOfficial: true,
  category: "official",
  icon: "deepseek",
};

const DSH_OPENAI_COMPATIBLE_PRESET: DeepSeekHarnessProviderPreset = {
  id: "dsh-openai-compatible",
  name: "OpenAI Compatible",
  websiteUrl: "https://api.openai.com",
  settingsConfig: {
    apiKey: "",
    baseURL: "https://api.openai.com/v1",
    displayName: "OpenAI Compatible",
    api: "openai-completions",
    apiKeyEnv: "OPENAI_API_KEY",
    models: [],
  },
  category: "custom",
  icon: "openai",
};

/**
 * OpenCode AI SDK packages mapped onto the DSH custom (pi-ai) protocol APIs.
 * `@ai-sdk/amazon-bedrock` / `@ai-sdk/google` fall back to the OpenAI-compatible
 * protocol; they are skipped anyway because they carry no `baseURL`.
 */
const OPENCODE_NPM_TO_DSH_API: Record<string, string> = {
  "@ai-sdk/openai-compatible": "openai-completions",
  "@ai-sdk/openai": "openai-responses",
  "@ai-sdk/anthropic": "anthropic-messages",
  "@ai-sdk/amazon-bedrock": "openai-completions",
  "@ai-sdk/google": "openai-completions",
};

/** Derives an uppercase credential reference name from a preset display name. */
function toApiKeyEnvBase(name: string): string {
  let ref = name
    .toUpperCase()
    .replace(/[^A-Z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
  if (!ref) ref = "DSH";
  if (/^[0-9]/.test(ref)) ref = `DSH_${ref}`;
  return `${ref}_API_KEY`;
}

/** Reserves a unique credential reference, appending `_2`, `_3`, ... on conflict. */
function reserveApiKeyEnv(name: string, usedRefs: Set<string>): string {
  const base = toApiKeyEnvBase(name);
  let ref = base;
  let counter = 2;
  while (usedRefs.has(ref)) {
    ref = `${base}_${counter}`;
    counter += 1;
  }
  usedRefs.add(ref);
  return ref;
}

/**
 * Preset ids can end up as native provider keys, and the backend only accepts
 * `[A-Za-z0-9._-]`; slugify so localized preset names stay valid.
 */
function toDshPresetId(name: string): string {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/^[-.]+|[-.]+$/g, "");
  return slug || "dsh-preset";
}

/**
 * Maps one OpenCode provider preset onto a DSH custom (pi-ai) preset. Returns
 * `null` for presets that have no meaningful DSH equivalent or are unsupported.
 */
export function toDeepSeekHarnessPreset(
  preset: OpenCodeProviderPreset,
  usedRefs: Set<string>,
): DeepSeekHarnessProviderPreset | null {
  // OMO templates manage agent model assignment, which DSH does not support.
  if (preset.category === "omo" || preset.category === "omo-slim") return null;
  // The official DeepSeek route is provided by DSH_OFFICIAL_PRESET.
  if (preset.name === "DeepSeek") return null;

  const npm = preset.settingsConfig.npm;
  if (!npm) return null;

  const api = OPENCODE_NPM_TO_DSH_API[npm];
  if (!api) return null;

  const rawBaseURL = preset.settingsConfig.options?.baseURL;
  if (typeof rawBaseURL !== "string" || rawBaseURL.trim() === "") {
    return null;
  }

  const models = Object.entries(preset.settingsConfig.models ?? {}).map(
    ([id, model]) => ({
      id,
      name: model?.name?.trim() || id,
    }),
  );

  return {
    id: toDshPresetId(preset.name),
    name: preset.name,
    nameKey: preset.nameKey,
    websiteUrl: preset.websiteUrl,
    apiKeyUrl: preset.apiKeyUrl,
    settingsConfig: {
      displayName: preset.name,
      api,
      baseURL: rawBaseURL,
      apiKeyEnv: reserveApiKeyEnv(preset.name, usedRefs),
      apiKey: "",
      models,
    },
    isOfficial: preset.isOfficial,
    category: preset.category,
    isPartner: preset.isPartner,
    primePartner: preset.primePartner,
    partnerPromotionKey: preset.partnerPromotionKey,
    templateValues: preset.templateValues,
    theme: preset.theme,
    icon: preset.icon,
    iconColor: preset.iconColor,
  };
}

/** Derives the DSH preset catalog from the OpenCode catalog, preserving order. */
function derivePresetsFromOpenCode(): DeepSeekHarnessProviderPreset[] {
  const usedRefs = new Set<string>();
  const derived: DeepSeekHarnessProviderPreset[] = [];
  for (const preset of opencodeProviderPresets) {
    const mapped = toDeepSeekHarnessPreset(preset, usedRefs);
    if (mapped) derived.push(mapped);
  }
  return derived;
}

export const deepseekHarnessProviderPresets: DeepSeekHarnessProviderPreset[] = [
  DSH_OFFICIAL_PRESET,
  DSH_OPENAI_COMPATIBLE_PRESET,
  ...derivePresetsFromOpenCode(),
];

export function getDeepSeekHarnessPresetEntries() {
  return deepseekHarnessProviderPresets.map((preset, index) => ({
    id: `deepseek-harness-${index}`,
    preset,
  }));
}
