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
 * cc-switch stores each provider in the ZCodeProviderConfig shape
 * { name, kind, options:{baseURL,apiKey}, enabled, source, models } and the
 * backend derives a `provider_config.json` personal rule from it: when the
 * baseURL matches a template in ZCode's builtin catalog
 * (resources/config/provider/zcode-builtin.json), the rule references that
 * templateId and inherits its endpoint + builtin models. So `kind` and
 * `baseURL` must match a real template, and `models` must list model IDs the
 * vendor actually serves — all values below mirror the ZCode 3.14 catalog.
 */
export const zcodeProviderPresets: ZCodeProviderPreset[] = [
  // ===== Z.AI / BigModel (智谱) — Anthropic-compatible coding endpoints =====
  {
    name: "Z.ai API Key",
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
        "GLM-5.3": { name: "GLM-5.3" },
        "GLM-5.3-Flash": { name: "GLM-5.3 Flash" },
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
        "GLM-5.3": { name: "GLM-5.3" },
        "GLM-5.3-Flash": { name: "GLM-5.3 Flash" },
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
        "kimi-k3": { name: "Kimi K3" },
        "kimi-k2.7-code": { name: "Kimi K2.7 Code" },
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
        "deepseek-flash": { name: "DeepSeek Flash" },
        "deepseek-v4-pro": { name: "DeepSeek V4 Pro" },
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
    websiteUrl: "https://bailian.console.aliyun.com",
    apiKeyUrl: "https://bailian.console.aliyun.com/#/api-key",
    settingsConfig: {
      name: "Qwen",
      kind: "anthropic",
      enabled: true,
      source: "custom",
      options: {
        baseURL: "https://dashscope.aliyuncs.com/apps/anthropic",
        apiKey: "",
      },
      models: {
        "qwen3.8-max": { name: "Qwen 3.8 Max" },
        "qwen3.8-flash": { name: "Qwen 3.8 Flash" },
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
        "mimo-v2.5": { name: "MiMo V2.5" },
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
