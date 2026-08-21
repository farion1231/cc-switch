import type { ProviderCategory } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

export interface DeepSeekHarnessModel {
  id: string;
  name: string;
}

export interface DeepSeekHarnessProviderConfig {
  apiKey?: string;
  baseURL?: string;
  profile?: string;
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

export const deepseekHarnessProviderPresets: DeepSeekHarnessProviderPreset[] = [
  {
    id: "deepseek-official",
    name: "DeepSeek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    settingsConfig: DEEPSEEK_HARNESS_DEFAULT_CONFIG,
    isOfficial: true,
    category: "official",
    icon: "deepseek",
  },
];

export function getDeepSeekHarnessPresetEntries() {
  return deepseekHarnessProviderPresets.map((preset, index) => ({
    id: `deepseek-harness-${index}`,
    preset,
  }));
}
