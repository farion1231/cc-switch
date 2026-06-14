/**
 * AtomCode provider presets configuration
 * AtomCode uses a flat settings object: { providerKey, type, model, api_key?, base_url?, context_window? }
 */
import type { ProviderCategory } from "../types";

export interface AtomcodeProviderSettingsConfig {
  providerKey: string;
  type: "openai" | "claude" | "ollama";
  model: string;
  api_key?: string;
  base_url?: string;
  context_window?: number;
}

export interface AtomcodeProviderPreset {
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: AtomcodeProviderSettingsConfig;
  category?: ProviderCategory;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  icon?: string;
  iconColor?: string;
}

export const atomcodeProviderPresets: AtomcodeProviderPreset[] = [
  {
    name: "DeepSeek",
    websiteUrl: "https://platform.deepseek.com/",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    settingsConfig: {
      providerKey: "deepseek",
      type: "openai",
      model: "deepseek-chat",
      base_url: "https://api.deepseek.com/v1",
      context_window: 64000,
    },
    category: "third_party",
  },
  {
    name: "Moonshot Kimi",
    websiteUrl: "https://platform.moonshot.cn/",
    settingsConfig: {
      providerKey: "kimi",
      type: "openai",
      model: "kimi-k2-0905-preview",
      base_url: "https://api.moonshot.cn/v1",
    },
    category: "third_party",
  },
  {
    name: "Claude Official",
    websiteUrl: "https://www.anthropic.com/",
    settingsConfig: {
      providerKey: "claude",
      type: "claude",
      model: "claude-opus-4-6",
      base_url: "https://api.anthropic.com",
    },
    category: "official",
  },
];
