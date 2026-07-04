import type { ProviderCategory, ZCodeProviderConfig } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

export interface ZCodeProviderPreset {
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: ZCodeProviderConfig;
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

export const zcodeProviderKinds = [
  { value: "openai-compatible", label: "OpenAI Compatible" },
  { value: "openai", label: "OpenAI" },
  { value: "anthropic", label: "Anthropic" },
] as const;

export const zcodeProviderPresets: ZCodeProviderPreset[] = [
  {
    name: "Custom OpenAI Compatible",
    nameKey: "providerPresets.customOpenAICompatible",
    websiteUrl: "",
    category: "custom",
    icon: "zcode",
    settingsConfig: {
      name: "Custom OpenAI Compatible",
      kind: "openai-compatible",
      options: {
        baseURL: "",
        apiKey: "",
        apiKeyRequired: true,
      },
      enabled: true,
      source: "custom",
      models: {},
    },
  },
  {
    name: "Z.ai API Key",
    websiteUrl: "https://api.z.ai",
    apiKeyUrl: "https://api.z.ai",
    category: "cn_official",
    icon: "zcode",
    settingsConfig: {
      name: "Z.ai - API Key",
      kind: "anthropic",
      options: {
        baseURL: "https://api.z.ai/api/anthropic",
        apiKey: "",
        apiKeyRequired: true,
      },
      enabled: true,
      source: "custom",
      models: {
        "GLM-5.2": {
          limit: { context: 1000000 },
          modalities: { input: ["text"], output: ["text"] },
        },
      },
    },
  },
  {
    name: "BigModel API Key",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://open.bigmodel.cn/usercenter/proj-mgmt/apikeys",
    category: "cn_official",
    icon: "chatglm",
    settingsConfig: {
      name: "Bigmodel - API Key",
      kind: "anthropic",
      options: {
        baseURL: "https://open.bigmodel.cn/api/anthropic",
        apiKey: "",
      },
      enabled: true,
      source: "custom",
      models: {
        "GLM-5.2": {
          limit: { context: 1000000 },
          modalities: { input: ["text"], output: ["text"] },
        },
      },
    },
  },
];
