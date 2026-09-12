import type { ProviderCategory, ZCodeProviderConfig } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

/**
 * ZCode provider "kind" options (config.json provider.<id>.kind).
 * Mirrors the values ZCode.app uses in its built-in model catalog.
 */
export const zcodeProviderKinds = [
  { value: "anthropic", label: "Anthropic" },
  { value: "openai-compatible", label: "OpenAI Compatible" },
  { value: "openai", label: "OpenAI" },
] as const;

export interface ZCodeProviderPreset {
  name: string;
  nameKey?: string; // i18n key for localized display name
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: ZCodeProviderConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
  primePartner?: boolean; // 置顶合作伙伴（顶级）：徽章显示为心形
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  templateValues?: Record<string, TemplateValueConfig>;
  theme?: PresetTheme;
  icon?: string;
  iconColor?: string;
  isCustomTemplate?: boolean;
}

/**
 * Official ZCode provider presets.
 *
 * Based on the built-in model catalog shipped with ZCode.app v3.3.6
 * (10 official providers observed). Each preset uses the ZCode
 * config.json shape: { name, kind, options:{baseURL,apiKey},
 * enabled, source:"custom", models:{<id>:{...}} }.
 */
export const zcodeProviderPresets: ZCodeProviderPreset[] = [
  // ===== Z.AI / BigModel (智谱) — Anthropic-compatible coding endpoints =====
  {
    name: "Z.ai API Key",
    nameKey: "providerForm.presets.zaiApiKey",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe",
    settingsConfig: {
      name: "Z.ai API Key",
      kind: "anthropic",
      enabled: true,
      source: "custom",
      options: {
        baseURL: "https://api.z.ai/api/anthropic",
        apiKey: "",
      },
      models: {
        "glm-5.2": { name: "GLM-5.2" },
        "glm-5-turbo": { name: "GLM-5-Turbo" },
      },
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
  },
  {
    name: "BigModel API Key",
    nameKey: "providerForm.presets.bigmodelApiKey",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://open.bigmodel.cn",
    settingsConfig: {
      name: "BigModel API Key",
      kind: "anthropic",
      enabled: true,
      source: "custom",
      options: {
        baseURL: "https://open.bigmodel.cn/api/anthropic",
        apiKey: "",
      },
      models: {
        "glm-5.2": { name: "GLM-5.2" },
        "glm-5-turbo": { name: "GLM-5-Turbo" },
      },
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
  },

  // ===== Other official CN providers observed in ZCode catalog =====
  {
    name: "Moonshot Kimi",
    nameKey: "providerForm.presets.moonshotKimi",
    websiteUrl: "https://platform.moonshot.cn",
    apiKeyUrl: "https://platform.moonshot.cn/console/api-keys",
    settingsConfig: {
      name: "Moonshot Kimi",
      kind: "anthropic",
      enabled: true,
      source: "custom",
      options: {
        baseURL: "https://api.moonshot.cn/anthropic",
        apiKey: "",
      },
      models: {
        "kimi-k2.6": { name: "Kimi K2.6" },
      },
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#1783FF",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
  },
  {
    name: "DeepSeek",
    nameKey: "providerForm.presets.deepseek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    settingsConfig: {
      name: "DeepSeek",
      kind: "anthropic",
      enabled: true,
      source: "custom",
      options: {
        baseURL: "https://api.deepseek.com/anthropic",
        apiKey: "",
      },
      models: {
        "deepseek-v4-flash": { name: "DeepSeek V4 Flash" },
      },
    },
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#4D6BFE",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
  },
  {
    name: "MiniMax",
    nameKey: "providerForm.presets.minimax",
    websiteUrl: "https://platform.minimaxi.com",
    apiKeyUrl: "https://platform.minimaxi.com/subscribe/coding-plan",
    settingsConfig: {
      name: "MiniMax",
      kind: "anthropic",
      enabled: true,
      source: "custom",
      options: {
        baseURL: "https://api.minimaxi.com/anthropic",
        apiKey: "",
      },
      models: {
        "MiniMax-M3": { name: "MiniMax M3" },
      },
    },
    category: "cn_official",
    icon: "minimax",
    iconColor: "#FF6B6B",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
  },
  {
    name: "Qwen",
    nameKey: "providerForm.presets.qwen",
    websiteUrl: "https://bailian.console.aliyun.com",
    apiKeyUrl: "https://bailian.console.aliyun.com/#/api-key",
    settingsConfig: {
      name: "Qwen",
      kind: "anthropic",
      enabled: true,
      source: "custom",
      options: {
        baseURL: "https://dashscope.aliyuncs.com/anthropic",
        apiKey: "",
      },
      models: {
        "qwen3.5-plus": { name: "Qwen 3.5 Plus" },
      },
    },
    category: "cn_official",
    icon: "bailian",
    iconColor: "#624AFF",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
  },
  {
    name: "Xiaomi MiMo",
    nameKey: "providerForm.presets.xiaomiMimo",
    websiteUrl: "https://platform.xiaomimimo.com",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/api-keys",
    settingsConfig: {
      name: "Xiaomi MiMo",
      kind: "anthropic",
      enabled: true,
      source: "custom",
      options: {
        baseURL: "https://api.xiaomimimo.com/anthropic",
        apiKey: "",
      },
      models: {
        "mimo-v2.5-pro": {
          name: "MiMo V2.5 Pro",
          limit: { context: 1048576, output: 131072 },
          modalities: { input: ["text"], output: ["text"] },
        },
      },
    },
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
  },

  // ===== Custom template =====
  {
    name: "Custom OpenAI-Compatible",
    nameKey: "providerForm.presets.customOpenAICompatible",
    websiteUrl: "",
    settingsConfig: {
      name: "Custom OpenAI-Compatible",
      kind: "openai-compatible",
      enabled: true,
      source: "custom",
      options: {
        baseURL: "",
        apiKey: "",
      },
      models: {},
    },
    category: "custom",
    icon: "zcode",
    iconColor: "#7C3AED",
    isCustomTemplate: true,
    templateValues: {
      baseURL: {
        label: "Base URL",
        placeholder: "https://api.example.com/v1",
        defaultValue: "",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
  },
];
