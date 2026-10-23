import type { ProviderCategory } from "@/types";
import type { PresetFamilyFields } from "./presetFamilies";
import type { PresetTheme } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";
import {
  extractCodexBaseUrl,
  extractCodexModelName,
} from "@/utils/providerConfigUtils";
import { TRAE_OFFICIAL_PROVIDER_ID } from "@/utils/providerCapabilities";

/** One `[model_providers.<key>]` table in `~/.trae/traecli.toml`. */
export interface TraeProviderTable {
  name?: string;
  base_url: string;
  wire_api?: "responses";
  experimental_bearer_token?: string;
  env_key?: string;
  [key: string]: unknown;
}

export type TraeProviderConfig =
  | { provider: TraeProviderTable; model: string }
  | { official: true };

export interface TraeProviderPreset extends PresetFamilyFields {
  name: string;
  nameKey?: string;
  providerKey: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  category?: ProviderCategory;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  icon?: string;
  iconColor?: string;
  theme?: PresetTheme;
  settingsConfig: TraeProviderConfig;
}

export const isTraeOfficialConfig = (
  config: unknown,
): config is { official: true } =>
  typeof config === "object" &&
  config !== null &&
  (config as { official?: unknown }).official === true;

/** Matches the backend rule: letters, digits, `-` and `_`. */
export const TRAE_PROVIDER_KEY_PATTERN = /^[A-Za-z0-9_-]+$/;
/** Provider ids built into traecli; redefining them is ignored upstream. */
export const TRAE_RESERVED_PROVIDER_KEYS = new Set([
  "openai",
  "oss",
  "ollama",
  "lmstudio",
  TRAE_OFFICIAL_PROVIDER_ID,
]);

export const normalizeTraeProviderKey = (value: string) =>
  value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "");

const officialPreset: TraeProviderPreset = {
  name: "TRAE",
  nameKey: "trae.officialPresetName",
  providerKey: TRAE_OFFICIAL_PROVIDER_ID,
  websiteUrl: "https://www.trae.cn",
  category: "official",
  icon: "trae",
  settingsConfig: { official: true },
};

/**
 * traecli (a Codex derivative) only accepts `wire_api = "responses"`, so only
 * Codex presets that talk to a native Responses endpoint carry over. Presets
 * that need CC Switch's Chat Completions conversion or an OAuth login do not.
 */
const derivedPresets: TraeProviderPreset[] = codexProviderPresets.flatMap(
  (preset) => {
    if (
      preset.hidden ||
      preset.isOfficial ||
      preset.isCustomTemplate ||
      preset.requiresOAuth ||
      preset.providerType ||
      preset.category === "official" ||
      preset.apiFormat !== "openai_responses"
    ) {
      return [];
    }
    const baseUrl = extractCodexBaseUrl(preset.config);
    const model = extractCodexModelName(preset.config);
    const normalized = normalizeTraeProviderKey(preset.name);
    // Built-in ids (openai, ollama, …) cannot be redefined in traecli.toml.
    const providerKey = TRAE_RESERVED_PROVIDER_KEYS.has(normalized)
      ? `${normalized}-custom`
      : normalized;
    if (!baseUrl || !model || !normalized) return [];
    return [
      {
        name: preset.name,
        nameKey: preset.nameKey,
        providerKey,
        websiteUrl: preset.websiteUrl,
        apiKeyUrl: preset.apiKeyUrl,
        category: preset.category,
        isPartner: preset.isPartner,
        partnerPromotionKey: preset.partnerPromotionKey,
        icon: preset.icon,
        iconColor: preset.iconColor,
        theme: preset.theme,
        family: preset.family,
        planKey: preset.planKey,
        regionKey: preset.regionKey,
        settingsConfig: {
          provider: {
            name: preset.name,
            base_url: baseUrl,
            wire_api: "responses",
            experimental_bearer_token: "",
          },
          model,
        },
      },
    ];
  },
);

export const traeProviderPresets: TraeProviderPreset[] = [
  officialPreset,
  ...derivedPresets,
];
