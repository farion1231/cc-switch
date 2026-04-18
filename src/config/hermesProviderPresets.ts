/**
 * Hermes Agent provider presets configuration
 * Hermes uses custom_providers array in config.yaml
 */
import type { ProviderCategory } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

/**
 * A model entry under a Hermes custom_provider.
 *
 * Serialized to YAML as a dict keyed by `id`:
 *
 * ```yaml
 * models:
 *   anthropic/claude-opus-4-7:
 *     context_length: 200000
 *     max_tokens: 32000
 * ```
 */
export interface HermesModel {
  /** Model ID — becomes the YAML key and the value written to top-level model.default. */
  id: string;
  /** Optional display label (UI only, not serialized to YAML). */
  name?: string;
  /** Override the auto-detected context window. */
  context_length?: number;
  /** Response-length cap. */
  max_tokens?: number;
}

/**
 * Top-level `model:` defaults suggested by a preset.
 *
 * Written to the YAML `model:` section when the user switches to this provider.
 * Per-model `context_length` / `max_tokens` live on the individual `HermesModel`
 * entries and flow through `custom_providers[].models`, not this object.
 */
export interface HermesSuggestedDefaults {
  model: {
    /** Model ID for `model.default`. Typically equals `models[0].id`. */
    default: string;
    /** Value for `model.provider`. Omit to use the custom_provider name. */
    provider?: string;
  };
}

/** Hermes custom_provider protocol mode (optional; auto-detected when omitted). */
export type HermesApiMode = "chat_completions" | "anthropic_messages";

/**
 * Form-facing value used by the API Mode dropdown.
 *
 * `auto` is the UI-only sentinel for "omit `api_mode` and let Hermes detect the
 * protocol from the endpoint". When serialized to `settings_config`, `auto`
 * becomes `undefined` so the YAML doesn't include `api_mode` at all.
 */
export type HermesApiModeChoice = "auto" | HermesApiMode;

/**
 * Dropdown options for the API Mode selector. `labelKey` is looked up in i18n;
 * `value` of "auto" means "don't write `api_mode` to the config".
 */
export const hermesApiModes: Array<{
  value: HermesApiModeChoice;
  labelKey: string;
}> = [
  { value: "auto", labelKey: "hermes.form.apiModeAuto" },
  { value: "chat_completions", labelKey: "hermes.form.apiModeChatCompletions" },
  {
    value: "anthropic_messages",
    labelKey: "hermes.form.apiModeAnthropicMessages",
  },
];

export interface HermesProviderPreset {
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: HermesProviderSettingsConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  templateValues?: Record<string, TemplateValueConfig>;
  theme?: PresetTheme;
  icon?: string;
  iconColor?: string;
  isCustomTemplate?: boolean;
  /** Optional top-level `model:` defaults written on switch. */
  suggestedDefaults?: HermesSuggestedDefaults;
}

export interface HermesProviderSettingsConfig {
  name: string;
  base_url?: string;
  api_key?: string;
  api_mode?: HermesApiMode;
  /** UI-side ordered list; serialized to YAML as a dict keyed by id. */
  models?: HermesModel[];
  [key: string]: unknown;
}

export const hermesProviderPresets: HermesProviderPreset[] = [
  {
    name: "OpenRouter",
    nameKey: "providerForm.presets.openrouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      name: "openrouter",
      base_url: "https://openrouter.ai/api/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "anthropic/claude-opus-4-7",
          name: "Claude Opus 4.7",
          context_length: 200000,
          max_tokens: 32000,
        },
        {
          id: "anthropic/claude-sonnet-4-6",
          name: "Claude Sonnet 4.6",
          context_length: 200000,
          max_tokens: 32000,
        },
        {
          id: "openai/gpt-5",
          name: "GPT-5",
          context_length: 400000,
        },
        {
          id: "google/gemini-3-pro",
          name: "Gemini 3 Pro",
          context_length: 1000000,
        },
      ],
    },
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6366F1",
    suggestedDefaults: {
      model: { default: "anthropic/claude-opus-4-7", provider: "openrouter" },
    },
  },
  {
    name: "Anthropic",
    nameKey: "providerForm.presets.anthropic",
    websiteUrl: "https://console.anthropic.com",
    apiKeyUrl: "https://console.anthropic.com/settings/keys",
    settingsConfig: {
      name: "anthropic",
      base_url: "https://api.anthropic.com",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-4-7",
          name: "Claude Opus 4.7",
          context_length: 200000,
          max_tokens: 32000,
        },
        {
          id: "claude-sonnet-4-6",
          name: "Claude Sonnet 4.6",
          context_length: 200000,
          max_tokens: 32000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          context_length: 200000,
          max_tokens: 16000,
        },
      ],
    },
    isOfficial: true,
    category: "official",
    icon: "anthropic",
    iconColor: "#D4915D",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "anthropic" },
    },
  },
  {
    name: "OpenAI",
    nameKey: "providerForm.presets.openai",
    websiteUrl: "https://platform.openai.com",
    apiKeyUrl: "https://platform.openai.com/api-keys",
    settingsConfig: {
      name: "openai",
      base_url: "https://api.openai.com/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "gpt-5",
          name: "GPT-5",
          context_length: 400000,
        },
        {
          id: "gpt-5-codex",
          name: "GPT-5 Codex",
          context_length: 400000,
        },
        {
          id: "o3-mini",
          name: "o3-mini",
          context_length: 200000,
        },
      ],
    },
    isOfficial: true,
    category: "official",
    icon: "openai",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "gpt-5", provider: "openai" },
    },
  },
  {
    name: "Google AI",
    nameKey: "providerForm.presets.googleai",
    websiteUrl: "https://ai.google.dev",
    apiKeyUrl: "https://aistudio.google.com/apikey",
    settingsConfig: {
      name: "google",
      api_key: "",
      models: [
        {
          id: "gemini-3-pro",
          name: "Gemini 3 Pro",
          context_length: 1000000,
        },
        {
          id: "gemini-3-flash",
          name: "Gemini 3 Flash",
          context_length: 1000000,
        },
      ],
    },
    isOfficial: true,
    category: "official",
    icon: "gemini",
    iconColor: "#4285F4",
    suggestedDefaults: {
      model: { default: "gemini-3-pro", provider: "google" },
    },
  },
  {
    name: "DeepSeek",
    nameKey: "providerForm.presets.deepseek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    settingsConfig: {
      name: "deepseek",
      base_url: "https://api.deepseek.com",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "deepseek-chat",
          name: "DeepSeek V3.2",
          context_length: 64000,
          max_tokens: 8000,
        },
        {
          id: "deepseek-reasoner",
          name: "DeepSeek R1",
          context_length: 64000,
          max_tokens: 8000,
        },
      ],
    },
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#4D6BFE",
    suggestedDefaults: {
      model: { default: "deepseek-chat", provider: "deepseek" },
    },
  },
  {
    name: "Together AI",
    nameKey: "providerForm.presets.together",
    websiteUrl: "https://together.ai",
    apiKeyUrl: "https://api.together.ai/settings/api-keys",
    settingsConfig: {
      name: "together",
      base_url: "https://api.together.xyz/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "Qwen/Qwen3-Coder-480B-A35B-Instruct",
          name: "Qwen3 Coder 480B",
          context_length: 262144,
        },
        {
          id: "deepseek-ai/DeepSeek-V3.2",
          name: "DeepSeek V3.2",
          context_length: 64000,
        },
        {
          id: "meta-llama/Llama-4-Maverick-17B-128E-Instruct-FP8",
          name: "Llama 4 Maverick",
          context_length: 131072,
        },
      ],
    },
    category: "aggregator",
    icon: "together",
    iconColor: "#0F6FFF",
    suggestedDefaults: {
      model: {
        default: "Qwen/Qwen3-Coder-480B-A35B-Instruct",
        provider: "together",
      },
    },
  },
  {
    name: "Nous Research",
    websiteUrl: "https://nousresearch.com",
    settingsConfig: {
      name: "nous",
      base_url: "https://inference.nous.hermes.dev/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "Hermes-4-405B",
          name: "Hermes 4 405B",
          context_length: 131072,
        },
        {
          id: "Hermes-4-70B",
          name: "Hermes 4 70B",
          context_length: 131072,
        },
      ],
    },
    isOfficial: true,
    category: "official",
    icon: "hermes",
    iconColor: "#7C3AED",
    suggestedDefaults: {
      model: { default: "Hermes-4-405B", provider: "nous" },
    },
  },
];
