import type { ProviderCategory } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

/**
 * 单条 WorkBuddy 模型（对应 `~/.workbuddy/models.json` 数组中的一个元素）。
 *
 * 已知字段显式列出，其余能力标记与元数据通过 `[key: string]: unknown`
 * 以 camelCase 原样透传，与 Rust 端 `WorkBuddyModelEntry` 的 flatten 逻辑一致。
 */
export interface WorkBuddyModel {
  /** 模型 ID（必填）。 */
  id: string;
  /** 可选别名（模型标识，如 `glm-5.1`），缺省时后端会用 `id` 兜底。 */
  model?: string;
  /** 可选显示名称。 */
  name?: string;
  /** 上下文窗口（tokens）。 */
  contextWindow?: number;
  /** 最大输出 tokens。 */
  maxTokens?: number;
  /** 是否支持工具调用。 */
  supportsToolCall?: boolean;
  /** 是否支持图片输入。 */
  supportsImages?: boolean;
  /** 是否支持推理。 */
  supportsReasoning?: boolean;
  /** 是否绕过本地代理。 */
  bypassProxy?: boolean;
  /** 是否使用自定义协议。 */
  useCustomProtocol?: boolean;
  /** 透传未来新增字段，避免升级 WorkBuddy 后字段丢失。 */
  [key: string]: unknown;
}

/**
 * WorkBuddy provider 的 settings_config（DB 存储，camelCase）。
 */
export interface WorkBuddyProviderSettingsConfig {
  /** 网关地址，如 `https://api.example.com/v1`。 */
  baseUrl: string;
  /** 该网关的 API Key。 */
  apiKey: string;
  /** 该网关下的模型清单。 */
  models?: WorkBuddyModel[];
  [key: string]: unknown;
}

export interface WorkBuddyProviderPreset {
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: WorkBuddyProviderSettingsConfig;
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

export const workbuddyProviderPresets: WorkBuddyProviderPreset[] = [
  {
    name: "OpenRouter",
    nameKey: "providerForm.presets.openrouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      baseUrl: "https://openrouter.ai/api/v1",
      apiKey: "",
      models: [
        {
          id: "anthropic/claude-opus-4-8",
          name: "Claude Opus 4.8",
          contextWindow: 1000000,
          supportsToolCall: true,
          supportsImages: true,
        },
        {
          id: "openai/gpt-5.5",
          name: "GPT-5.5",
          contextWindow: 400000,
          supportsToolCall: true,
          supportsImages: true,
        },
      ],
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
      baseUrl: "https://api.deepseek.com",
      apiKey: "",
      models: [
        {
          id: "deepseek-v4-pro",
          name: "DeepSeek V4 Pro",
          contextWindow: 1000000,
        },
        {
          id: "deepseek-v4-flash",
          name: "DeepSeek V4 Flash",
          contextWindow: 1000000,
        },
      ],
    },
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#4D6BFE",
  },
  {
    name: "SiliconFlow",
    websiteUrl: "https://siliconflow.cn",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/YflgU2Ve",
    settingsConfig: {
      baseUrl: "https://api.siliconflow.cn/v1",
      apiKey: "",
      models: [
        {
          id: "Pro/MiniMaxAI/MiniMax-M2.7",
          name: "Pro / MiniMax M2.7",
          supportsToolCall: true,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "siliconflow",
    icon: "siliconflow",
    iconColor: "#6E29F6",
  },
];
