import type { ProviderCategory } from "@/types";
import type { PresetTheme } from "./claudeProviderPresets";

export interface PiPresetModel {
  id: string;
  name?: string;
  reasoning?: boolean;
  input?: Array<"text" | "image">;
  contextWindow?: number;
  maxTokens?: number;
}

export interface PiProviderPreset {
  name: string;
  nameKey?: string;
  providerKey: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: {
    name: string;
    baseUrl: string;
    api: string;
    apiKey: string;
    models: PiPresetModel[];
  };
  category?: ProviderCategory;
  isPartner?: boolean;
  primePartner?: boolean;
  partnerPromotionKey?: string;
  theme?: PresetTheme;
  icon?: string;
  iconColor?: string;
}

/**
 * A deliberately small Pi catalog. These reuse the same commercial ordering,
 * URLs, icons and protocol choices already maintained by the other apps.
 * Native Pi providers are intentionally absent: Pi owns their login state.
 */
export const piProviderPresets: PiProviderPreset[] = [
  {
    name: "Kimi",
    providerKey: "cc-switch-kimi",
    primePartner: true,
    websiteUrl: "https://platform.kimi.com?aff=cc-switch",
    apiKeyUrl: "https://platform.kimi.com/console/api-keys?aff=cc-switch",
    settingsConfig: {
      name: "Kimi",
      baseUrl: "https://api.moonshot.cn/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        {
          id: "kimi-k2.7-code",
          name: "Kimi K2.7 Code",
          reasoning: true,
          input: ["text", "image"],
        },
        {
          id: "kimi-k3",
          name: "Kimi K3",
          reasoning: true,
          input: ["text", "image"],
        },
      ],
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "Kimi For Coding",
    providerKey: "cc-switch-kimi-coding",
    primePartner: true,
    websiteUrl: "https://www.kimi.com/code/?aff=cc-switch",
    apiKeyUrl: "https://platform.kimi.com/console/api-keys?aff=cc-switch",
    settingsConfig: {
      name: "Kimi For Coding",
      baseUrl: "https://api.kimi.com/coding/",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        {
          id: "kimi-for-coding",
          name: "Kimi For Coding",
          reasoning: true,
          input: ["text", "image"],
        },
      ],
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "PackyCode",
    providerKey: "cc-switch-packycode",
    websiteUrl: "https://www.packyapi.ai",
    apiKeyUrl: "https://www.packyapi.ai/register?aff=cc-switch",
    settingsConfig: {
      name: "PackyCode",
      baseUrl: "https://www.packyapi.ai",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          reasoning: true,
          input: ["text", "image"],
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          reasoning: true,
          input: ["text", "image"],
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "packycode",
    icon: "packycode",
  },
  {
    name: "ZetaAPI",
    providerKey: "cc-switch-zetaapi",
    websiteUrl: "https://zetaapi.ai",
    apiKeyUrl: "https://zetaapi.ai/go/u117",
    settingsConfig: {
      name: "ZetaAPI",
      baseUrl: "https://api.zetaapi.ai/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          reasoning: true,
          input: ["text", "image"],
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "zetaapi",
    icon: "zetaapi",
  },
  {
    name: "APINebula",
    providerKey: "cc-switch-apinebula",
    websiteUrl: "https://apinebula.ai",
    apiKeyUrl: "https://apinebula.ai/VjM74M",
    settingsConfig: {
      name: "APINebula",
      baseUrl: "https://apinebula.ai/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          reasoning: true,
          input: ["text", "image"],
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apinebula",
    icon: "apinebula",
  },
  {
    name: "AICodeMirror",
    providerKey: "cc-switch-aicodemirror",
    websiteUrl: "https://www.aicodemirror.ai",
    apiKeyUrl: "https://www.aicodemirror.ai/register?invitecode=9915W3",
    settingsConfig: {
      name: "AICodeMirror",
      baseUrl: "https://api.aicodemirror.ai/api/claudecode",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          reasoning: true,
          input: ["text", "image"],
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          reasoning: true,
          input: ["text", "image"],
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicodemirror",
    icon: "aicodemirror",
    iconColor: "#000000",
  },
  {
    name: "OpenRouter",
    nameKey: "providerForm.presets.openrouter",
    providerKey: "cc-switch-openrouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      name: "OpenRouter",
      baseUrl: "https://openrouter.ai/api/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        {
          id: "anthropic/claude-opus-5",
          name: "Claude Opus 5",
          reasoning: true,
          input: ["text", "image"],
        },
        {
          id: "openai/gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          reasoning: true,
          input: ["text", "image"],
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
    providerKey: "cc-switch-deepseek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    settingsConfig: {
      name: "DeepSeek",
      baseUrl: "https://api.deepseek.com",
      api: "openai-completions",
      apiKey: "",
      models: [
        {
          id: "deepseek-v4-flash",
          name: "DeepSeek V4 Flash",
          reasoning: true,
          input: ["text"],
        },
        {
          id: "deepseek-v4-pro",
          name: "DeepSeek V4 Pro",
          reasoning: true,
          input: ["text"],
        },
      ],
    },
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#4D6BFE",
  },
];
