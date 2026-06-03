/**
 * Kimi Code CLI 预设供应商配置模板
 */
import { ProviderCategory } from "../types";

export interface KimiProviderPreset {
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: object;
  endpointCandidates?: string[];
  isOfficial?: boolean;
  category?: ProviderCategory;
  icon?: string;
  iconColor?: string;
}

export const kimiProviderPresets: KimiProviderPreset[] = [
  {
    name: "Kimi Official",
    websiteUrl: "https://platform.moonshot.cn/console",
    apiKeyUrl: "https://platform.moonshot.cn/console/api-keys",
    settingsConfig: {
      env: {
        KIMI_BASE_URL: "https://api.moonshot.cn/v1",
        KIMI_API_KEY: "",
        KIMI_PROVIDER_NAME: "ccswitch",
      },
    },
    endpointCandidates: ["https://api.moonshot.cn/v1"],
    isOfficial: true,
    category: "official",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "Kimi For Coding",
    websiteUrl: "https://www.kimi.com/code/docs/",
    apiKeyUrl: "https://platform.moonshot.cn/console/api-keys",
    settingsConfig: {
      env: {
        KIMI_BASE_URL: "https://api.kimi.com/coding/v1",
        KIMI_API_KEY: "",
        KIMI_PROVIDER_NAME: "ccswitch",
      },
    },
    endpointCandidates: ["https://api.kimi.com/coding/v1"],
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
  },
];
