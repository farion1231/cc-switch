/**
 * Pi provider presets configuration
 * Pi uses additive mode (all providers coexist in models.json `providers`,
 * like OpenCode/OpenClaw/Hermes).
 * Settings format: env-based with api type, similar to Claude/Gemini presets
 * Pi models.json spec: https://pi.dev/docs/latest/models
 */
import type { ProviderCategory } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

/**
 * Pi API protocol types.
 * See https://pi.dev/docs/latest/models for the full list.
 */
export type PiApiType =
  | "anthropic-messages"
  | "openai-completions"
  | "openai-responses"
  | "google-generative-ai";

export const PI_API_TYPES: { value: PiApiType; label: string }[] = [
  { value: "anthropic-messages", label: "Anthropic Messages" },
  { value: "openai-completions", label: "OpenAI Completions" },
  { value: "openai-responses", label: "OpenAI Responses" },
  { value: "google-generative-ai", label: "Google Generative AI" },
];

/**
 * Pi model entry (per https://pi.dev/docs/latest/models).
 * Only `id` is required; other fields are optional metadata.
 */
export interface PiModel {
  id: string;
  name?: string;
  contextWindow?: number;
  maxTokens?: number;
  reasoning?: boolean;
  cost?: {
    input: number;
    output: number;
    cacheRead?: number;
    cacheWrite?: number;
  };
}

export interface PiProviderPreset {
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: object;
  isOfficial?: boolean;
  isPartner?: boolean;
  primePartner?: boolean;
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  templateValues?: Record<string, TemplateValueConfig>;
  theme?: PresetTheme;
  icon?: string;
  iconColor?: string;
  isCustomTemplate?: boolean;
}

export const piProviderPresets: PiProviderPreset[] = [
  {
    name: "Anthropic Official",
    websiteUrl: "https://www.anthropic.com",
    apiKeyUrl: "https://console.anthropic.com/settings/keys",
    settingsConfig: {
      api: "anthropic-messages",
      env: {},
      models: [
        { id: "claude-sonnet-4-20250514", name: "Claude Sonnet 4" },
        { id: "claude-opus-4-20250514", name: "Claude Opus 4" },
      ] as PiModel[],
    },
    isOfficial: true,
    category: "official",
    theme: {
      backgroundColor: "#D97757",
      textColor: "#FFFFFF",
    },
    icon: "anthropic",
    iconColor: "#D4915D",
  },
  {
    name: "OpenAI Official",
    websiteUrl: "https://platform.openai.com",
    apiKeyUrl: "https://platform.openai.com/api-keys",
    settingsConfig: {
      api: "openai-completions",
      env: {
        OPENAI_BASE_URL: "https://api.openai.com/v1",
        OPENAI_API_KEY: "",
      },
      models: [
        { id: "gpt-4o", name: "GPT-4o" },
        { id: "gpt-4.1", name: "GPT-4.1" },
      ] as PiModel[],
    },
    isOfficial: true,
    category: "official",
    theme: {
      backgroundColor: "#10A37F",
      textColor: "#FFFFFF",
    },
    icon: "openai",
    iconColor: "#00A67E",
  },
  {
    name: "OpenRouter",
    nameKey: "providerForm.presets.openrouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      api: "anthropic-messages",
      env: {
        ANTHROPIC_BASE_URL: "https://openrouter.ai/api/v1",
        ANTHROPIC_API_KEY: "",
      },
      models: [] as PiModel[],
    },
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6366F1",
  },
  {
    name: "DeepSeek",
    nameKey: "providerForm.presets.deepseek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    settingsConfig: {
      api: "openai-completions",
      env: {
        OPENAI_BASE_URL: "https://api.deepseek.com",
        OPENAI_API_KEY: "",
      },
      models: [
        { id: "deepseek-chat", name: "DeepSeek Chat" },
        { id: "deepseek-reasoner", name: "DeepSeek Reasoner" },
      ] as PiModel[],
    },
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#4D6BFE",
  },
  {
    name: "Custom Endpoint",
    websiteUrl: "",
    settingsConfig: {
      api: "anthropic-messages",
      env: {
        ANTHROPIC_BASE_URL: "",
        ANTHROPIC_API_KEY: "",
      },
      models: [] as PiModel[],
    },
    category: "custom",
    isCustomTemplate: true,
    templateValues: {
      ANTHROPIC_BASE_URL: {
        label: "Base URL",
        placeholder: "https://your-api-endpoint.com/v1",
        defaultValue: "",
        editorValue: "",
      },
    },
  },
];
