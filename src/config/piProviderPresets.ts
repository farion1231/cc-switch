/**
 * Pi Coding Agent provider presets configuration
 * Pi uses models.json providers structure with per-provider configs
 */
import type { ProviderCategory } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

/** Pi provider config structure (matches PiProviderConfig in types.ts) */
export interface PiProviderPresetConfig {
  baseUrl?: string;
  apiKey?: string;
  api?: string; // "anthropic-messages" | "openai-completions" | "openai-responses" | "google-generative-ai"
  models?: Array<{
    id: string;
    name?: string;
    reasoning?: boolean;
    input?: string[];
    contextWindow?: number;
    maxTokens?: number;
  }>;
  [key: string]: unknown;
}

export interface PiProviderPreset {
  name: string;
  nameKey?: string;
  /** Suggested models.json provider key (lowercase kebab-case) */
  providerKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  /** Pi settings_config structure */
  settingsConfig: PiProviderPresetConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
  primePartner?: boolean;
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  /** Template variable definitions */
  templateValues?: Record<string, TemplateValueConfig>;
  /** Visual theme config */
  theme?: PresetTheme;
  /** Icon name */
  icon?: string;
  /** Icon color */
  iconColor?: string;
  /** Mark as custom template (for UI distinction) */
  isCustomTemplate?: boolean;
}

/** Pi 默认配置（JSON 序列化后用于 ProviderForm 的初始值） */
export const PI_DEFAULT_CONFIG = JSON.stringify(
  {
    baseUrl: "",
    apiKey: "",
    api: "openai-completions",
    models: [],
  },
  null,
  2,
);

/**
 * Pi provider presets list
 */
export const piProviderPresets: PiProviderPreset[] = [
  // Pi 没有浏览器 OAuth 登录：所有预设一律走 API Key。
  // 禁止使用 category:"official" / isOfficial（那会触发「官方登录无需 Key」文案并禁用输入框）。
  // ==========================================================================
  // 平台官方（API Key）
  // ==========================================================================
  {
    name: "OpenAI",
    providerKey: "openai",
    websiteUrl: "https://platform.openai.com",
    apiKeyUrl: "https://platform.openai.com/api-keys",
    category: "cn_official",
    settingsConfig: {
      baseUrl: "https://api.openai.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        { id: "gpt-4o", name: "GPT-4o", reasoning: false, input: ["text", "image"] },
        { id: "gpt-4o-mini", name: "GPT-4o Mini", reasoning: false, input: ["text", "image"] },
        { id: "o3", name: "o3", reasoning: true, input: ["text"] },
        { id: "o4-mini", name: "o4 Mini", reasoning: true, input: ["text"] },
      ],
    },
    icon: "openai",
    iconColor: "#00A67E",
  },
  {
    name: "Anthropic",
    providerKey: "anthropic",
    websiteUrl: "https://console.anthropic.com",
    apiKeyUrl: "https://console.anthropic.com/settings/keys",
    category: "cn_official",
    settingsConfig: {
      baseUrl: "https://api.anthropic.com",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        { id: "claude-sonnet-4-20250514", name: "Claude 4 Sonnet", reasoning: true, input: ["text", "image"] },
        { id: "claude-sonnet-4-20250514-thinking", name: "Claude 4 Sonnet (Extended Thinking)", reasoning: true, input: ["text", "image"] },
      ],
    },
    icon: "claude",
    iconColor: "#CC9B7A",
  },
  {
    name: "Google Gemini",
    providerKey: "google-gemini",
    websiteUrl: "https://ai.google.dev",
    apiKeyUrl: "https://aistudio.google.com/app/apikey",
    category: "cn_official",
    settingsConfig: {
      baseUrl: "https://generativelanguage.googleapis.com/v1beta",
      apiKey: "",
      api: "google-generative-ai",
      models: [
        { id: "gemini-2.5-pro-preview-05-06", name: "Gemini 2.5 Pro", reasoning: true, input: ["text", "image"] },
        { id: "gemini-2.5-flash-preview-05-06", name: "Gemini 2.5 Flash", reasoning: true, input: ["text", "image"] },
      ],
    },
    icon: "gemini",
    iconColor: "#4285F4",
  },
  {
    name: "DeepSeek",
    nameKey: "providerForm.presets.deepseek",
    providerKey: "deepseek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    category: "cn_official",
    settingsConfig: {
      baseUrl: "https://api.deepseek.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        { id: "deepseek-chat", name: "DeepSeek V3", reasoning: false, input: ["text"] },
        { id: "deepseek-reasoner", name: "DeepSeek R1", reasoning: true, input: ["text"] },
      ],
    },
    icon: "deepseek",
    iconColor: "#4D6BFE",
  },
  {
    name: "Zhipu GLM",
    providerKey: "zhipu-glm",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    category: "cn_official",
    settingsConfig: {
      baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4",
      apiKey: "",
      api: "openai-completions",
      models: [
        { id: "glm-5.2", name: "GLM-5.2", reasoning: false, input: ["text", "image"] },
      ],
    },
    icon: "zhipu",
    iconColor: "#4B8BFF",
  },
  {
    name: "Zhipu GLM en",
    providerKey: "zhipu-glm-en",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe?ic=8JVLJQFSKB",
    category: "cn_official",
    settingsConfig: {
      baseUrl: "https://api.z.ai/api/coding/paas/v4",
      apiKey: "",
      api: "openai-completions",
      models: [
        { id: "glm-5.2", name: "GLM-5.2", reasoning: false, input: ["text", "image"] },
      ],
    },
    icon: "zhipu",
    iconColor: "#4B8BFF",
  },
  {
    name: "火山Agentplan",
    providerKey: "volcengine",
    websiteUrl: "https://www.volcengine.com/activity/codingplan",
    apiKeyUrl: "https://www.volcengine.com/activity/codingplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    category: "cn_official",
    settingsConfig: {
      baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
      apiKey: "",
      api: "openai-completions",
      models: [
        { id: "ark-code-latest", name: "Ark Code Latest", reasoning: false, input: ["text"] },
      ],
    },
    icon: "volcengine",
    iconColor: "#0077FF",
  },
  {
    name: "BytePlus",
    providerKey: "byteplus",
    websiteUrl: "https://www.byteplus.com/en/product/modelark",
    apiKeyUrl: "https://www.byteplus.com/en/product/modelark?utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    category: "cn_official",
    settingsConfig: {
      baseUrl: "https://ark.ap-southeast.bytepluses.com/api/coding/v3",
      apiKey: "",
      api: "openai-completions",
      models: [
        { id: "ark-code-latest", name: "Ark Code Latest", reasoning: false, input: ["text"] },
      ],
    },
    icon: "volcengine",
    iconColor: "#0077FF",
  },
  // ==========================================================================
  // 聚合供应商
  // ==========================================================================
  {
    name: "OpenRouter",
    nameKey: "providerForm.presets.openrouter",
    providerKey: "openrouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    category: "aggregator",
    settingsConfig: {
      baseUrl: "https://openrouter.ai/api/v1",
      apiKey: "",
      api: "openai-completions",
      models: [],
    },
    icon: "openrouter",
    iconColor: "#FF6B35",
  },
  // ==========================================================================
  // 第三方供应商
  // ==========================================================================
  {
    name: "Longcat",
    providerKey: "longcat",
    websiteUrl: "https://longcat.chat/platform",
    apiKeyUrl: "https://longcat.chat/platform/api_keys",
    category: "third_party",
    settingsConfig: {
      baseUrl: "https://api.longcat.chat/anthropic",
      apiKey: "",
      api: "anthropic-messages",
      models: [],
    },
    icon: "longcat",
    iconColor: "#FF6B6B",
  },
];
