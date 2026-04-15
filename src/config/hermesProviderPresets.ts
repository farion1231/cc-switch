/**
 * Hermes Agent provider presets configuration
 * Hermes uses custom_providers array in config.yaml
 */
import type { ProviderCategory } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

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
}

export interface HermesProviderSettingsConfig {
  name: string;
  base_url?: string;
  api_key?: string;
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
    },
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6366F1",
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
    },
    isOfficial: true,
    category: "official",
    icon: "anthropic",
    iconColor: "#D4915D",
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
    },
    isOfficial: true,
    category: "official",
    icon: "openai",
    iconColor: "#000000",
  },
  {
    name: "Google AI",
    nameKey: "providerForm.presets.googleai",
    websiteUrl: "https://ai.google.dev",
    apiKeyUrl: "https://aistudio.google.com/apikey",
    settingsConfig: {
      name: "google",
      api_key: "",
    },
    isOfficial: true,
    category: "official",
    icon: "gemini",
    iconColor: "#4285F4",
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
    },
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#4D6BFE",
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
    },
    category: "aggregator",
    icon: "together",
    iconColor: "#0F6FFF",
  },
  {
    name: "Nous Research",
    websiteUrl: "https://nousresearch.com",
    settingsConfig: {
      name: "nous",
      base_url: "https://inference.nous.hermes.dev/v1",
      api_key: "",
    },
    isOfficial: true,
    category: "official",
    icon: "hermes",
    iconColor: "#7C3AED",
  },
];
