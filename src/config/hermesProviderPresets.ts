/**
 * Hermes provider presets
 */
import type { ProviderPreset } from "./claudeProviderPresets";

export type HermesProviderPreset = ProviderPreset;

export const hermesProviderPresets: HermesProviderPreset[] = [
  {
    name: "Anthropic Official",
    websiteUrl: "https://www.anthropic.com",
    settingsConfig: {
      provider: "anthropic",
      apiKey: "",
      model: "claude-sonnet-4.6",
    },
    isOfficial: true,
    category: "official",
    icon: "anthropic",
    iconColor: "#D4915D",
  },
  {
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      provider: "openrouter",
      baseUrl: "https://openrouter.ai/api/v1",
      apiKey: "",
      model: "anthropic/claude-sonnet-4.6",
    },
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6566F1",
  },
  {
    name: "Custom",
    websiteUrl: "",
    settingsConfig: {
      provider: "",
      baseUrl: "",
      apiKey: "",
      model: "",
    },
    category: "custom",
  },
];
