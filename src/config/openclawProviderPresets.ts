/**
 * OpenClaw provider presets configuration
 * OpenClaw uses models.providers structure with custom provider configs
 * 费用为标准 USD / 百万 tokens 估算，优先使用供应商公布的单价。
 * 套餐和中转参考价不包含订阅费用或账户折扣。
 */
import type {
  ProviderCategory,
  OpenClawProviderConfig,
  OpenClawDefaultModel,
} from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";
import type { PresetFamilyFields } from "./presetFamilies";

/** Suggested default model configuration for a preset */
export interface OpenClawSuggestedDefaults {
  /** Default model config to apply (agents.defaults.model) */
  model?: OpenClawDefaultModel;
  /** Model catalog entries to add (agents.defaults.models) */
  modelCatalog?: Record<string, { alias?: string }>;
}

export interface OpenClawProviderPreset extends PresetFamilyFields {
  name: string;
  nameKey?: string; // i18n key for localized display name
  websiteUrl: string;
  apiKeyUrl?: string;
  /** OpenClaw settings_config structure */
  settingsConfig: OpenClawProviderConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
  primePartner?: boolean; // 旧版的置顶合作伙伴标记；v7 起界面不再读取，新预设不写
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
  /** Suggested default model configuration */
  suggestedDefaults?: OpenClawSuggestedDefaults;
}

function rebaseOpenClawModelRef(modelRef: string, providerKey: string): string {
  const slashIndex = modelRef.indexOf("/");
  return slashIndex === -1
    ? `${providerKey}/${modelRef}`
    : `${providerKey}${modelRef.slice(slashIndex)}`;
}

/**
 * OpenClaw default model refs are stored as "<provider-key>/<model-id>".
 * Presets carry stable built-in keys for display/tests, but the real key is
 * chosen in the add-provider form, so rewrite refs right before submission.
 */
export function rebaseOpenClawSuggestedDefaults(
  defaults: OpenClawSuggestedDefaults,
  providerKey: string,
): OpenClawSuggestedDefaults {
  const key = providerKey.trim();
  if (!key) return defaults;

  return {
    model: defaults.model
      ? {
          ...defaults.model,
          primary: rebaseOpenClawModelRef(defaults.model.primary, key),
          fallbacks: defaults.model.fallbacks?.map((modelRef) =>
            rebaseOpenClawModelRef(modelRef, key),
          ),
        }
      : undefined,
    modelCatalog: defaults.modelCatalog
      ? Object.fromEntries(
          Object.entries(defaults.modelCatalog).map(([modelRef, entry]) => [
            rebaseOpenClawModelRef(modelRef, key),
            entry,
          ]),
        )
      : undefined,
  };
}

/**
 * OpenClaw API protocol options
 * @see https://github.com/openclaw/openclaw/blob/main/docs/gateway/configuration.md
 */
export const openclawApiProtocols = [
  { value: "openai-completions", label: "OpenAI Completions" },
  { value: "openai-responses", label: "OpenAI Responses" },
  { value: "anthropic-messages", label: "Anthropic Messages" },
  { value: "google-generative-ai", label: "Google Generative AI" },
  { value: "bedrock-converse-stream", label: "AWS Bedrock" },
] as const;

/**
 * OpenClaw provider presets list
 */
export const openclawProviderPresets: OpenClawProviderPreset[] = [
  // ===== 赞助商预设：文件顺序与 README 赞助商表对齐（仅维护约定；应用内一律按显示名排序，不置顶）=====
  {
    name: "Kimi",
    family: "kimi",
    planKey: "payg",
    regionKey: "cn",
    primePartner: true,
    websiteUrl:
      "https://platform.kimi.com?track_id=track-7cf2b91dcde043eda6ef9a95951a042c&aff=cc-switch",
    apiKeyUrl: "https://platform.kimi.com/console/api-keys?aff=cc-switch",
    settingsConfig: {
      baseUrl: "https://api.moonshot.cn/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "kimi-k2.7-code",
          name: "Kimi K2.7 Code",
          contextWindow: 262144,
          cost: { input: 0.95, output: 4, cacheRead: 0.19 },
        },
        {
          id: "kimi-k3",
          name: "Kimi K3",
          contextWindow: 1048576,
          cost: { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 0 },
        },
        {
          id: "kimi-k2.7-code-highspeed",
          name: "Kimi K2.7 Code HighSpeed",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 262144,
          maxTokens: 262144,
          cost: { input: 1.9, output: 8, cacheRead: 0.38 },
        },
        {
          id: "kimi-k2.6",
          name: "Kimi K2.6",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 262144,
          maxTokens: 262144,
          cost: { input: 0.95, output: 4, cacheRead: 0.16 },
        },
      ],
    },
    category: "cn_official",
    partnerPromotionKey: "kimi",
    icon: "kimi",
    iconColor: "#6366F1",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.moonshot.cn/v1",
        defaultValue: "https://api.moonshot.cn/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "kimi/kimi-k2.7-code" },
      modelCatalog: { "kimi/kimi-k2.7-code": { alias: "Kimi" } },
    },
  },
  // API 开放平台海外/Global 变体：platform.kimi.ai + api.moonshot.ai 端点
  {
    name: "Kimi Global",
    family: "kimi",
    planKey: "payg",
    regionKey: "intl",
    websiteUrl:
      "https://platform.kimi.ai?track_id=track-674ed6e2af924a5682a87421f7cf753a&aff=cc-switch",
    apiKeyUrl: "https://platform.kimi.ai/console/api-keys?aff=cc-switch",
    settingsConfig: {
      baseUrl: "https://api.moonshot.ai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "kimi-k2.7-code",
          name: "Kimi K2.7 Code",
          contextWindow: 262144,
          cost: { input: 0.95, output: 4, cacheRead: 0.19 },
        },
        {
          id: "kimi-k3",
          name: "Kimi K3",
          contextWindow: 1048576,
          cost: { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 0 },
        },
        {
          id: "kimi-k2.7-code-highspeed",
          name: "Kimi K2.7 Code HighSpeed",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 262144,
          maxTokens: 262144,
          cost: { input: 1.9, output: 8, cacheRead: 0.38 },
        },
        {
          id: "kimi-k2.6",
          name: "Kimi K2.6",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 262144,
          maxTokens: 262144,
          cost: { input: 0.95, output: 4, cacheRead: 0.16 },
        },
      ],
    },
    category: "cn_official",
    partnerPromotionKey: "kimi",
    icon: "kimi",
    iconColor: "#6366F1",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.moonshot.ai/v1",
        defaultValue: "https://api.moonshot.ai/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "kimi/kimi-k2.7-code" },
      modelCatalog: { "kimi/kimi-k2.7-code": { alias: "Kimi" } },
    },
  },
  {
    name: "Kimi For Coding",
    family: "kimi",
    planKey: "coding",
    regionKey: "cn",
    primePartner: true,
    websiteUrl: "https://www.kimi.com/code/?aff=cc-switch",
    apiKeyUrl: "https://platform.kimi.com/console/api-keys?aff=cc-switch",
    settingsConfig: {
      baseUrl: "https://api.kimi.com/coding/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "kimi-for-coding",
          name: "Kimi For Coding",
          contextWindow: 131072,
          cost: { input: 0.95, output: 4, cacheRead: 0.19 },
        },
      ],
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.kimi.com/coding/v1",
        defaultValue: "https://api.kimi.com/coding/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "kimi-coding/kimi-for-coding" },
      modelCatalog: { "kimi-coding/kimi-for-coding": { alias: "Kimi" } },
    },
  },
  // 海外/Global 变体：kimi.ai/code + api.kimi.ai 端点，其余与国内版一致
  {
    name: "Kimi For Coding Global",
    family: "kimi",
    planKey: "coding",
    regionKey: "intl",
    websiteUrl: "https://www.kimi.ai/code?aff=cc-switch",
    apiKeyUrl: "https://www.kimi.ai/code?aff=cc-switch",
    settingsConfig: {
      baseUrl: "https://api.kimi.ai/coding/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "kimi-for-coding",
          name: "Kimi For Coding",
          contextWindow: 131072,
          cost: { input: 0.95, output: 4, cacheRead: 0.19 },
        },
      ],
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.kimi.ai/coding/v1",
        defaultValue: "https://api.kimi.ai/coding/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "kimi-coding/kimi-for-coding" },
      modelCatalog: { "kimi-coding/kimi-for-coding": { alias: "Kimi" } },
    },
  },

  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.ai",
    apiKeyUrl: "https://www.packyapi.ai/register?aff=cc-switch",
    settingsConfig: {
      baseUrl: "https://www.packyapi.ai",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 4, output: 20, cacheRead: 0.2, cacheWrite: 5 },
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 0.25, cacheWrite: 12.5 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "packycode",
    icon: "packycode",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "packycode/claude-opus-5",
        fallbacks: ["packycode/claude-sonnet-5"],
      },
      modelCatalog: {
        "packycode/claude-opus-5": { alias: "Opus" },
        "packycode/claude-sonnet-5": { alias: "Sonnet" },
      },
    },
  },
  {
    name: "ZetaAPI",
    websiteUrl: "https://zetaapi.ai",
    apiKeyUrl: "https://zetaapi.ai/go/u117",
    settingsConfig: {
      baseUrl: "https://api.zetaapi.ai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          contextWindow: 400000,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "zetaapi",
    icon: "zetaapi",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "zetaapi/gpt-5.6-sol",
      },
      modelCatalog: {
        "zetaapi/gpt-5.6-sol": { alias: "GPT-5.6 Sol" },
      },
    },
  },
  {
    name: "APINebula",
    websiteUrl: "https://apinebula.ai",
    apiKeyUrl: "https://apinebula.ai/VjM74M",
    settingsConfig: {
      baseUrl: "https://apinebula.ai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
        },
        {
          id: "gpt-6-astra",
          name: "GPT-6 Astra",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 1, cacheWrite: 12.5 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apinebula",
    icon: "apinebula",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "apinebula/gpt-5.6-sol",
      },
    },
  },
  {
    name: "AICodeMirror",
    websiteUrl: "https://www.aicodemirror.ai",
    apiKeyUrl: "https://www.aicodemirror.ai/register?invitecode=9915W3",
    settingsConfig: {
      baseUrl: "https://api.aicodemirror.ai/api/claudecode",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicodemirror",
    icon: "aicodemirror",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "aicodemirror/claude-opus-5",
        fallbacks: ["aicodemirror/claude-sonnet-5"],
      },
      modelCatalog: {
        "aicodemirror/claude-opus-5": { alias: "Opus" },
        "aicodemirror/claude-sonnet-5": { alias: "Sonnet" },
      },
    },
  },
  {
    name: "FennoAI",
    websiteUrl: "https://api.fenno.ai",
    apiKeyUrl:
      "https://api.fenno.ai/register?redirect=/purchase?tab=subscription%26group=16&aff=P9MR3D3PLCNL",
    settingsConfig: {
      baseUrl: "https://api.fenno.ai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          contextWindow: 400000,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "fenno",
    icon: "fenno",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "fenno/gpt-5.6-sol",
      },
      modelCatalog: {
        "fenno/gpt-5.6-sol": { alias: "GPT-5.6 Sol" },
      },
    },
  },
  {
    name: "RunAPI",
    websiteUrl: "https://runapi.host",
    apiKeyUrl: "https://runapi.host/register?aff=iOKB",
    settingsConfig: {
      baseUrl: "https://runapi.host",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
        },
        {
          id: "claude-haiku-4-5",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 4, output: 20, cacheRead: 0.2, cacheWrite: 5 },
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 0.25, cacheWrite: 12.5 },
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "runapi",
    icon: "runapi",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "runapi/claude-sonnet-5",
      },
      modelCatalog: {
        "runapi/claude-opus-5": { alias: "Opus" },
        "runapi/claude-sonnet-5": { alias: "Sonnet" },
        "runapi/claude-haiku-4-5": { alias: "Haiku" },
      },
    },
  },
  {
    name: "OpenModel",
    websiteUrl: "https://www.openmodel.ai",
    apiKeyUrl: "https://www.openmodel.ai?ref=cc-switch",
    settingsConfig: {
      baseUrl: "https://api.openmodel.ai",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
      ],
    },
    category: "aggregator",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "openmodel/claude-opus-5-5",
        fallbacks: ["openmodel/claude-sonnet-5-5"],
      },
      modelCatalog: {
        "openmodel/claude-opus-5-5": { alias: "Opus" },
        "openmodel/claude-sonnet-5-5": { alias: "Sonnet" },
        "openmodel/claude-haiku-4-5-20251001": { alias: "Haiku" },
      },
    },
    isPartner: true,
    icon: "openmodel",
  },
  {
    name: "GrooRoute Pro",
    websiteUrl: "https://grooroute.pro",
    apiKeyUrl: "https://grooroute.pro/sign-up?aff=qqm9",
    settingsConfig: {
      baseUrl: "https://grooroute.pro",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
      ],
    },
    category: "aggregator",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "grooroute/claude-opus-5-5",
        fallbacks: ["grooroute/claude-sonnet-5-5"],
      },
      modelCatalog: {
        "grooroute/claude-opus-5-5": { alias: "Opus" },
        "grooroute/claude-sonnet-5-5": { alias: "Sonnet" },
      },
    },
    isPartner: true,
    icon: "grooroute",
  },
  {
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    settingsConfig: {
      baseUrl: "https://router.shengsuanyun.com/api",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "anthropic/claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
        {
          id: "anthropic/claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
        {
          id: "anthropic/claude-opus-5.5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 4, output: 20, cacheRead: 0.2, cacheWrite: 5 },
        },
        {
          id: "anthropic/claude-fable-5.1",
          name: "Claude Fable 5.1",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 0.25, cacheWrite: 12.5 },
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "shengsuanyun",
    icon: "shengsuanyun",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "shengsuanyun/anthropic/claude-opus-5",
        fallbacks: ["shengsuanyun/anthropic/claude-sonnet-5"],
      },
      modelCatalog: {
        "shengsuanyun/anthropic/claude-opus-5": { alias: "Opus" },
        "shengsuanyun/anthropic/claude-sonnet-5": { alias: "Sonnet" },
      },
    },
  },
  {
    name: "AIGoCode",
    websiteUrl: "https://aigocode.net",
    apiKeyUrl: "https://aigocode.net/invite/CC-SWITCH",
    settingsConfig: {
      baseUrl: "https://api.aigocode.dev",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aigocode",
    icon: "aigocode",
    iconColor: "#5B7FFF",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "aigocode/claude-opus-5",
        fallbacks: ["aigocode/claude-sonnet-5"],
      },
      modelCatalog: {
        "aigocode/claude-opus-5": { alias: "Opus" },
        "aigocode/claude-sonnet-5": { alias: "Sonnet" },
      },
    },
  },
  {
    name: "Qiniu",
    nameKey: "providerForm.presets.qiniu",
    websiteUrl: "https://s.qiniu.com/nMvAvy",
    apiKeyUrl: "https://s.qiniu.com/nMvAvy",
    settingsConfig: {
      baseUrl: "https://api.qnaigc.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "gpt-6-astra",
          name: "GPT-6 Astra",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 1, cacheWrite: 12.5 },
        },
        {
          id: "moonshotai/kimi-k3",
          name: "Kimi K3",
          contextWindow: 1048576,
          maxTokens: 131072,
          input: ["text", "image"],
          reasoning: true,
        },
        {
          id: "z-ai/glm-5.3",
          name: "GLM-5.3",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 131072,
        },
        {
          id: "z-ai/glm-5.3-flash",
          name: "GLM-5.3-Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "qiniu",
    icon: "qiniu",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "qiniu/gpt-6-astra",
      },
      modelCatalog: {
        "qiniu/gpt-6-astra": { alias: "GPT-6 Astra" },
      },
    },
  },
  {
    name: "AICoding",
    websiteUrl: "https://aicoding.inc",
    apiKeyUrl: "https://aicoding.inc/i/CCSWITCH",
    settingsConfig: {
      baseUrl: "https://api.aicoding.inc",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicoding",
    icon: "aicoding",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "aicoding/claude-opus-5",
        fallbacks: ["aicoding/claude-sonnet-5"],
      },
      modelCatalog: {
        "aicoding/claude-opus-5": { alias: "Opus" },
        "aicoding/claude-sonnet-5": { alias: "Sonnet" },
      },
    },
  },
  {
    name: "SubRouter",
    websiteUrl: "https://subrouter.ai",
    apiKeyUrl: "https://subrouter.ai/register?aff=l3ri",
    settingsConfig: {
      baseUrl: "https://subrouter.ai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          contextWindow: 400000,
        },
        {
          id: "gpt-6-astra",
          name: "GPT-6 Astra",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 1, cacheWrite: 12.5 },
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "subrouter",
    icon: "subrouter",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "subrouter/gpt-5.6-sol",
      },
      modelCatalog: {
        "subrouter/gpt-5.6-sol": { alias: "GPT-5.6 Sol" },
      },
    },
  },
  {
    // FluxA AgentMarket 以合作价转售的百度智能云 TokenPlan：产品页写明
    // "purchase it through AgentMarket, then use Baidu AI Cloud's endpoint and
    // API key directly"，端点取其所链的百度国际站 Token Plan Enterprise 文档
    // （2026-09-16 版）team 专属基址 —— 与国内个人版 qianfan.baidubce.com/
    // .../personal 是两套部署，勿合并。阵容与窗口按 FluxA 产品页模型表
    // （glm-5.2 500k ≠ 国内版千帆平台 1M，国际 team 部署口径，勿按国内预设
    // "修正"）；标注 Coming soon 的 deepseek-v4-pro-0813 / glm-5.3 不收。
    // Kimi K2.6 是定稿赞助文案点名的模型，FluxA 产品页模型表与百度国际站
    // team 文档都没列它：id / 窗口取 FluxA baidu-ai-cloud 模型目录（categories
    // 只有 text）与国内 Token Plan 预设（262144）双重印证，非臆造；
    // maxTokens 取同一张表的 Max output 列。cost 不填：FluxA 按 credits 计费、
    // 未公布每百万 token 单价，不造数
    name: "FluxA Token Plan",
    websiteUrl: "https://agentmarket.fluxapay.xyz/",
    apiKeyUrl: "https://agentmarket.fluxapay.xyz/marketplace/tokenplans",
    settingsConfig: {
      baseUrl: "https://api.baiduqianfan.ai/v2/tokenplan/team",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "deepseek-v4-pro",
          name: "DeepSeek V4 Pro",
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 393216,
        },
        {
          id: "deepseek-v4-flash-0731",
          name: "DeepSeek V4 Flash 0731",
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 393216,
        },
        {
          id: "deepseek-v4-flash",
          name: "DeepSeek V4 Flash",
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 393216,
        },
        {
          id: "deepseek-v3.2",
          name: "DeepSeek V3.2",
          input: ["text"],
          contextWindow: 131072,
          maxTokens: 32768,
        },
        {
          id: "glm-5.2",
          name: "GLM-5.2",
          input: ["text"],
          contextWindow: 500000,
          maxTokens: 131072,
        },
        {
          id: "glm-5.1",
          name: "GLM-5.1",
          input: ["text"],
          contextWindow: 198000,
          maxTokens: 131072,
        },
        {
          id: "glm-5",
          name: "GLM-5",
          input: ["text"],
          contextWindow: 198000,
          maxTokens: 131072,
        },
        {
          // FluxA 产品页模型表无 Kimi 行 → maxTokens 照仓库内既有 kimi-k2.6
          // 条目（262144），勿按 GLM 行的 131072 推
          id: "kimi-k2.6",
          name: "Kimi K2.6",
          input: ["text"],
          contextWindow: 262144,
          maxTokens: 262144,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "fluxa",
    icon: "fluxa",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "fluxa-tokenplan/deepseek-v4-pro" },
      modelCatalog: {
        "fluxa-tokenplan/deepseek-v4-pro": { alias: "DeepSeek V4 Pro" },
        "fluxa-tokenplan/glm-5.2": { alias: "GLM-5.2" },
      },
    },
  },
  {
    name: "88API",
    websiteUrl: "https://88api.ai",
    apiKeyUrl: "https://88api.ai/sign-up?aff=HSGY",
    settingsConfig: {
      baseUrl: "https://api.88api.ai",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-haiku-4-5",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "88api",
    icon: "88api",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "88api/claude-opus-5-5",
        fallbacks: ["88api/claude-sonnet-5-5"],
      },
      modelCatalog: {
        "88api/claude-opus-5-5": { alias: "Opus" },
        "88api/claude-sonnet-5-5": { alias: "Sonnet" },
        "88api/claude-haiku-4-5": { alias: "Haiku" },
      },
    },
  },
  {
    name: "APIKEY.FUN",
    websiteUrl: "https://apikey.fan",
    apiKeyUrl: "https://apikey.fan/register?aff=CCSwitch",
    settingsConfig: {
      baseUrl: "https://api.apikey.fan",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
        },
        {
          id: "claude-haiku-4-5",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apikeyfun",
    icon: "apikeyfun",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "apikeyfun/claude-opus-5",
        fallbacks: ["apikeyfun/claude-sonnet-5"],
      },
      modelCatalog: {
        "apikeyfun/claude-opus-5": { alias: "Opus" },
        "apikeyfun/claude-sonnet-5": { alias: "Sonnet" },
        "apikeyfun/claude-haiku-4-5": { alias: "Haiku" },
      },
    },
  },
  {
    name: "9527CODE",
    websiteUrl: "https://9527.codes",
    apiKeyUrl: "https://9527.codes/register?aff=e5zI",
    settingsConfig: {
      baseUrl: "https://9527.codes",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
        },
        {
          id: "claude-haiku-4-5",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 4, output: 20, cacheRead: 0.2, cacheWrite: 5 },
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 0.25, cacheWrite: 12.5 },
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "9527code",
    icon: "9527code",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "9527code/claude-opus-5",
        fallbacks: ["9527code/claude-sonnet-5"],
      },
      modelCatalog: {
        "9527code/claude-opus-5": { alias: "Opus" },
        "9527code/claude-sonnet-5": { alias: "Sonnet" },
        "9527code/claude-haiku-4-5": { alias: "Haiku" },
      },
    },
  },
  {
    name: "Code0",
    websiteUrl: "https://code0.ai",
    apiKeyUrl: "https://code0.ai/agent/register/B2XHxGjGmRvqgznY",
    settingsConfig: {
      baseUrl: "https://code0.ai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          contextWindow: 400000,
        },
        {
          id: "gpt-6-astra",
          name: "GPT-6 Astra",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 1, cacheWrite: 12.5 },
        },
      ],
    },
    category: "aggregator",
    icon: "code0",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "code0/gpt-5.6-sol",
      },
      modelCatalog: {
        "code0/gpt-5.6-sol": { alias: "GPT-5.6 Sol" },
      },
    },
  },
  {
    name: "TeamoRouter",
    websiteUrl: "https://teamorouter.cn",
    apiKeyUrl:
      "https://teamorouter.cn/?utm_source=cc_switch&utm_medium=referral&utm_campaign=ai_directory",
    settingsConfig: {
      baseUrl: "https://api.teamorouter.cn/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          contextWindow: 400000,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "teamorouter",
    icon: "teamorouter",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "teamorouter/gpt-5.6-sol",
      },
      modelCatalog: {
        "teamorouter/gpt-5.6-sol": { alias: "GPT-5.6 Sol" },
      },
    },
  },
  {
    name: "PPIO",
    websiteUrl: "https://ppio.com",
    apiKeyUrl: "https://ppio.com/activity/ccswitch",
    settingsConfig: {
      baseUrl: "https://api.ppio.com/openai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "deepseek/deepseek-v4-flash-0731",
          name: "Deepseek V4 Flash 0731",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 393216,
          cost: { input: 0.14, output: 0.29, cacheRead: 0.03 },
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ppio",
    icon: "ppio",
    iconColor: "#2874FF",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "ppio/deepseek/deepseek-v4-flash-0731" },
      modelCatalog: {
        "ppio/deepseek/deepseek-v4-flash-0731": {
          alias: "Deepseek V4 Flash 0731",
        },
      },
    },
  },
  {
    name: "ClaudeCN",
    websiteUrl: "https://claudecn.top",
    apiKeyUrl: "https://claudecn.ai/register?aff=HEL9",
    settingsConfig: {
      baseUrl: "https://claudecn.top",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
        },
        {
          id: "claude-haiku-4-5",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 4, output: 20, cacheRead: 0.2, cacheWrite: 5 },
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 0.25, cacheWrite: 12.5 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "claudecn",
    icon: "claudecn",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "claudecn/claude-sonnet-5",
      },
      modelCatalog: {
        "claudecn/claude-opus-5": { alias: "Opus" },
        "claudecn/claude-sonnet-5": { alias: "Sonnet" },
        "claudecn/claude-haiku-4-5": { alias: "Haiku" },
      },
    },
  },
  {
    name: "火山 Agent Plan",
    family: "volcengine",
    planKey: "agentPlan",
    websiteUrl:
      "https://www.volcengine.com/activity/agentplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_source=OWO&utm_medium=devrel-1&utm_campaign=hw&utm_term=ccswitch&utm_content=hw",
    apiKeyUrl:
      "https://www.volcengine.com/activity/agentplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_source=OWO&utm_medium=devrel-1&utm_campaign=hw&utm_term=ccswitch&utm_content=hw",
    settingsConfig: {
      baseUrl: "https://ark.cn-beijing.volces.com/api/plan/v3",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "ark-code-latest",
          name: "Ark Code Latest",
          contextWindow: 256000,
        },
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "volcengine_agentplan",
    icon: "huoshan",
    iconColor: "#3370FF",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "ark_agentplan/ark-code-latest" },
      modelCatalog: {
        "ark_agentplan/ark-code-latest": { alias: "Ark Code" },
      },
    },
  },
  {
    name: "火山 Coding Plan",
    family: "volcengine",
    planKey: "codingPlan",
    websiteUrl:
      "https://www.volcengine.com/activity/codingplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://www.volcengine.com/activity/codingplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    settingsConfig: {
      baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "ark-code-latest",
          name: "Ark Code Latest",
          contextWindow: 256000,
        },
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "volcengine_codingplan",
    icon: "huoshan",
    iconColor: "#3370FF",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "ark_codingplan/ark-code-latest" },
      modelCatalog: {
        "ark_codingplan/ark-code-latest": { alias: "Ark Code" },
      },
    },
  },
  {
    name: "BytePlus",
    websiteUrl:
      "https://www.byteplus.com/en/product/modelark?utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://www.byteplus.com/en/product/modelark?utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    settingsConfig: {
      baseUrl: "https://ark.ap-southeast.bytepluses.com/api/coding/v3",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "ark-code-latest",
          name: "Ark Code Latest",
          contextWindow: 256000,
        },
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "byteplus",
    icon: "byteplus",
    iconColor: "#3370FF",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "byteplus/ark-code-latest" },
      modelCatalog: {
        "byteplus/ark-code-latest": { alias: "Ark Code" },
      },
    },
  },
  {
    name: "Volcengine Doubao",
    family: "volcengine",
    planKey: "payg",
    nameKey: "providerForm.presets.doubaoseed",
    websiteUrl:
      "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey?apikey=%7B%7D&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey?apikey=%7B%7D&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    settingsConfig: {
      baseUrl: "https://ark.cn-beijing.volces.com/api/v3",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "doubao-seed-2-1-pro-260628",
          name: "DouBao Seed 2.1 Pro",
          contextWindow: 262144,
          cost: { input: 0.84, output: 4.2 },
        },
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "doubaoseed",
    icon: "doubao",
    iconColor: "#3370FF",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "doubaoseed/doubao-seed-2-1-pro-260628" },
      modelCatalog: {
        "doubaoseed/doubao-seed-2-1-pro-260628": { alias: "DouBao" },
      },
    },
  },
  {
    name: "SiliconFlow",
    family: "siliconflow",
    regionKey: "cn",
    websiteUrl: "https://siliconflow.cn",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/YflgU2Ve",
    settingsConfig: {
      baseUrl: "https://api.siliconflow.cn/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "Pro/MiniMaxAI/MiniMax-M2.5",
          name: "MiniMax M2.5",
          contextWindow: 196608,
          cost: { input: 0.3, output: 1.2, cacheRead: 0.06, cacheWrite: 0.375 },
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "siliconflow",
    icon: "siliconflow",
    iconColor: "#6E29F6",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "siliconflow/Pro/MiniMaxAI/MiniMax-M2.5" },
      modelCatalog: {
        "siliconflow/Pro/MiniMaxAI/MiniMax-M2.5": { alias: "MiniMax" },
      },
    },
  },
  {
    name: "SiliconFlow en",
    family: "siliconflow",
    regionKey: "intl",
    websiteUrl: "https://siliconflow.com",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/YflgU2Ve",
    settingsConfig: {
      baseUrl: "https://api.siliconflow.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "MiniMaxAI/MiniMax-M3",
          name: "MiniMax M3",
          contextWindow: 1048576,
          cost: { input: 0.3, output: 1.2, cacheRead: 0.06, cacheWrite: 0.375 },
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "siliconflow",
    icon: "siliconflow",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "siliconflow-en/MiniMaxAI/MiniMax-M3" },
      modelCatalog: {
        "siliconflow-en/MiniMaxAI/MiniMax-M3": { alias: "MiniMax" },
      },
    },
  },
  {
    name: "A6API",
    websiteUrl: "https://www.a6api.com",
    apiKeyUrl: "https://a6api.com/register?aff=AqNr",
    settingsConfig: {
      baseUrl: "https://api.a6api.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          contextWindow: 400000,
        },
        {
          id: "gpt-6-astra",
          name: "GPT-6 Astra",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 1, cacheWrite: 12.5 },
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "a6api",
    icon: "a6api",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "a6api/gpt-5.6-sol",
      },
      modelCatalog: {
        "a6api/gpt-5.6-sol": { alias: "GPT-5.6 Sol" },
      },
    },
  },
  {
    name: "Compshare",
    family: "compshare",
    planKey: "payg",
    nameKey: "providerForm.presets.ucloud",
    websiteUrl: "https://www.compshare.cn",
    apiKeyUrl:
      "https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch",
    settingsConfig: {
      baseUrl: "https://api.modelverse.cn/v1",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
      ],
    },
    category: "aggregator",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "ucloud", // 促销信息 i18n key
    icon: "ucloud",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "compshare/claude-opus-5",
      },
      modelCatalog: {
        "compshare/claude-opus-5": { alias: "Opus" },
      },
    },
  },
  {
    name: "Compshare Coding Plan",
    family: "compshare",
    planKey: "codingPlan",
    nameKey: "providerForm.presets.ucloudCoding",
    websiteUrl: "https://www.compshare.cn",
    apiKeyUrl:
      "https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch",
    settingsConfig: {
      baseUrl: "https://cp.compshare.cn/v1",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
      ],
    },
    category: "aggregator",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "ucloud", // 促销信息 i18n key（复用）
    icon: "ucloud",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "compshare-coding/claude-opus-5",
      },
      modelCatalog: {
        "compshare-coding/claude-opus-5": { alias: "Opus" },
      },
    },
  },
  {
    name: "CCSub",
    websiteUrl: "https://www.ccsub.net",
    apiKeyUrl: "https://www.ccsub.net/register?ref=Y6Z8DXEA",
    settingsConfig: {
      baseUrl: "https://www.ccsub.net/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "gpt-6-astra",
          name: "GPT-6 Astra",
          contextWindow: 1050000,
          cost: { input: 10, output: 50, cacheRead: 1, cacheWrite: 12.5 },
          maxTokens: 128000,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ccsub",
    icon: "ccsub",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "ccsub/gpt-6-astra",
      },
      modelCatalog: {
        "ccsub/gpt-6-astra": { alias: "GPT-6 Astra" },
      },
    },
  },
  {
    name: "SSSAiCode",
    websiteUrl: "https://sssaicodeapi.com",
    apiKeyUrl: "https://sssaicodeapi.com/register?ref=DCP0SM",
    settingsConfig: {
      baseUrl: "https://node-hk.sssaicodeapi.com/api",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "sssaicode",
    icon: "sssaicode",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "sssaicode/claude-opus-5",
        fallbacks: ["sssaicode/claude-sonnet-5"],
      },
      modelCatalog: {
        "sssaicode/claude-opus-5": { alias: "Opus" },
        "sssaicode/claude-sonnet-5": { alias: "Sonnet" },
      },
    },
  },
  {
    name: "SoleAPI",
    websiteUrl: "https://soleapi.com",
    apiKeyUrl: "https://soleapi.com/r/ccswitch",
    settingsConfig: {
      baseUrl: "https://soleapi.com",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "soleapi",
    icon: "soleapi",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "soleapi/claude-opus-5",
        fallbacks: ["soleapi/claude-sonnet-5"],
      },
      modelCatalog: {
        "soleapi/claude-opus-5": { alias: "Opus" },
        "soleapi/claude-sonnet-5": { alias: "Sonnet" },
        "soleapi/claude-haiku-4-5-20251001": { alias: "Haiku" },
      },
    },
  },
  {
    name: "Micu",
    websiteUrl: "https://www.micuapi.ai",
    apiKeyUrl: "https://www.micuapi.ai/register?aff=aOYQ",
    settingsConfig: {
      baseUrl: "https://www.micuapi.ai",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "micu",
    icon: "micu",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "micu/claude-opus-5",
      },
      modelCatalog: {
        "micu/claude-opus-5": { alias: "Opus" },
      },
    },
  },
  {
    name: "RightCode",
    websiteUrl: "https://www.rightapi.ai",
    apiKeyUrl: "https://www.rightapi.ai/register?aff=CCSWITCH",
    settingsConfig: {
      baseUrl: "https://www.rightapi.ai/claude",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "rightcode",
    icon: "rc",
    iconColor: "#E96B2C",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "rightcode/claude-opus-5",
        fallbacks: ["rightcode/claude-sonnet-5"],
      },
      modelCatalog: {
        "rightcode/claude-opus-5": { alias: "Opus" },
        "rightcode/claude-sonnet-5": { alias: "Sonnet" },
      },
    },
  },
  {
    name: "Cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    settingsConfig: {
      baseUrl: "https://api.cubence.com",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "cubence",
    icon: "cubence",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "cubence/claude-opus-5",
        fallbacks: ["cubence/claude-sonnet-5"],
      },
      modelCatalog: {
        "cubence/claude-opus-5": { alias: "Opus" },
        "cubence/claude-sonnet-5": { alias: "Sonnet" },
      },
    },
  },
  {
    name: "CrazyRouter",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    settingsConfig: {
      baseUrl: "https://cn.crazyrouter.com/v1",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 0.25, cacheWrite: 12.5 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "crazyrouter",
    icon: "crazyrouter",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "crazyrouter/claude-opus-5",
        fallbacks: ["crazyrouter/claude-sonnet-5"],
      },
      modelCatalog: {
        "crazyrouter/claude-opus-5": { alias: "Opus" },
        "crazyrouter/claude-sonnet-5": { alias: "Sonnet" },
      },
    },
  },
  {
    name: "DMXAPI",
    websiteUrl: "https://www.dmxapi.cn",
    apiKeyUrl: "https://www.dmxapi.cn",
    settingsConfig: {
      baseUrl: "https://www.dmxapi.cn",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "dmxapi",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "dmxapi/claude-opus-5",
        fallbacks: ["dmxapi/claude-sonnet-5"],
      },
      modelCatalog: {
        "dmxapi/claude-opus-5": { alias: "Opus" },
        "dmxapi/claude-sonnet-5": { alias: "Sonnet" },
      },
    },
    icon: "dmxapi",
  },
  {
    name: "SudoCode.chat",
    family: "sudocode",
    websiteUrl: "https://sudocode.chat",
    apiKeyUrl:
      "https://sudocode.chat/sign-up?aff=CC-SWITCH&utm_source=cc-switch&utm_medium=sponsor&utm_campaign=ccswitch",
    settingsConfig: {
      baseUrl: "https://api.sudocode.chat/v1",
      apiKey: "",
      api: "openai-responses",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
        },
        {
          id: "gpt-6-sol",
          name: "GPT-6 Sol",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
        {
          id: "gpt-6-luna",
          name: "GPT-6 Luna",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 0.1, output: 0.5, cacheRead: 0.01, cacheWrite: 0.125 },
        },
        {
          id: "gpt-6-astra",
          name: "GPT-6 Astra",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 1, cacheWrite: 12.5 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "sudocode",
    icon: "sudocode",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "sudocode/gpt-5.6-sol",
      },
    },
  },
  {
    name: "SudoCode.us",
    family: "sudocode",
    websiteUrl: "https://sudocode.us",
    apiKeyUrl: "https://sudocode.us",
    settingsConfig: {
      baseUrl: "https://sudocode.us/v1",
      apiKey: "",
      api: "openai-responses",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
        },
        {
          id: "gpt-6-sol",
          name: "GPT-6 Sol",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
        {
          id: "gpt-6-astra",
          name: "GPT-6 Astra",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 1, cacheWrite: 12.5 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    icon: "sudocode-us",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "sudocode-us/gpt-5.6-sol",
      },
    },
  },
  {
    name: "XycAi",
    websiteUrl: "https://xycai.us",
    apiKeyUrl: "https://xycai.us/register?aff=Uhu9",
    settingsConfig: {
      baseUrl: "https://apicdn.xycai.us/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          contextWindow: 400000,
        },
        {
          id: "gpt-6-astra",
          name: "GPT-6 Astra",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 1, cacheWrite: 12.5 },
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "xycai",
    icon: "xycai",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "xycai/gpt-5.6-sol",
      },
      modelCatalog: {
        "xycai/gpt-5.6-sol": { alias: "GPT-5.6 Sol" },
      },
    },
  },
  // ===== 非赞助商预设：应用内展示按显示名排序，此处文件顺序不影响展示 =====
  {
    name: "Tu-zi",
    nameKey: "providerForm.presets.tuzi",
    websiteUrl: "https://api.tu-zi.com",
    apiKeyUrl: "https://api.tu-zi.com/token",
    settingsConfig: {
      baseUrl: "https://api.tu-zi.com",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
        },
        {
          id: "claude-haiku-4-5",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
      ],
    },
    category: "aggregator",
    icon: "tuzi",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "tuzi/claude-opus-5",
        fallbacks: ["tuzi/claude-sonnet-5"],
      },
      modelCatalog: {
        "tuzi/claude-opus-5": { alias: "Opus" },
        "tuzi/claude-sonnet-5": { alias: "Sonnet" },
        "tuzi/claude-haiku-4-5": { alias: "Haiku" },
      },
    },
  },
  {
    name: "Amux",
    websiteUrl: "https://amux.ai",
    apiKeyUrl: "https://amux.ai",
    settingsConfig: {
      baseUrl: "https://api.amux.ai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          contextWindow: 400000,
        },
        {
          id: "gpt-6-astra",
          name: "GPT-6 Astra",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 1, cacheWrite: 12.5 },
        },
      ],
    },
    category: "aggregator",
    icon: "amux",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "amux/gpt-5.6-sol",
      },
      modelCatalog: {
        "amux/gpt-5.6-sol": { alias: "GPT-5.6 Sol" },
      },
    },
  },
  {
    name: "AtlasCloud",
    websiteUrl: "https://www.atlascloud.ai/console/coding-plan",
    apiKeyUrl: "https://www.atlascloud.ai/console/coding-plan",
    settingsConfig: {
      baseUrl: "https://api.atlascloud.ai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "zai-org/glm-5.1",
          name: "GLM 5.1",
        },
      ],
    },
    category: "aggregator",
    icon: "atlascloud",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "atlascloud/zai-org/glm-5.1",
      },
    },
  },
  {
    // 平台 OpenClaw 指南：anthropic-messages 协议 baseUrl 不带 /v1（指南里
    // "去掉 /v2 后缀"是笔误，/v2/* 实测全 404）。定价取模型市场标价（USD）。
    name: "Soshow",
    websiteUrl: "https://aimarket.so-show.com",
    apiKeyUrl: "https://aimarket.so-show.com/workbench/access-key",
    settingsConfig: {
      baseUrl: "https://maas.so-show.com",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25 },
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: { input: 3, output: 15 },
        },
      ],
    },
    category: "aggregator",
    icon: "soshow",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "soshow/claude-opus-5",
        fallbacks: ["soshow/claude-sonnet-5"],
      },
      modelCatalog: {
        "soshow/claude-opus-5": { alias: "Opus" },
        "soshow/claude-sonnet-5": { alias: "Sonnet" },
      },
    },
  },
  {
    name: "DeepSeek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    settingsConfig: {
      baseUrl: "https://api.deepseek.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "deepseek-v4-pro",
          name: "DeepSeek V4 Pro",
          contextWindow: 1000000,
          cost: { input: 1.32, output: 3.96, cacheRead: 0.044, cacheWrite: 0 },
        },
        {
          id: "deepseek-flash",
          name: "DeepSeek V4.1 Flash",
          contextWindow: 1000000,
          cost: { input: 0.3, output: 1.2, cacheRead: 0.006, cacheWrite: 0 },
          input: ["text", "image"],
          reasoning: true,
          maxTokens: 384000,
        },
      ],
    },
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#1E88E5",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "deepseek/deepseek-flash",
        fallbacks: ["deepseek/deepseek-v4-pro"],
      },
      modelCatalog: {
        "deepseek/deepseek-flash": { alias: "Flash" },
        "deepseek/deepseek-v4-pro": { alias: "Pro" },
      },
    },
  },
  {
    name: "Zhipu GLM",
    family: "zhipu",
    regionKey: "cn",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    settingsConfig: {
      baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "glm-5.3",
          name: "GLM-5.3",
          contextWindow: 1048576,
          cost: { input: 1.4, output: 4.4, cacheRead: 0.26 },
        },
        {
          id: "glm-5.3-flash",
          name: "GLM-5.3-Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 0.15, output: 0.5, cacheRead: 0.03 },
        },
      ],
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://open.bigmodel.cn/api/coding/paas/v4",
        defaultValue: "https://open.bigmodel.cn/api/coding/paas/v4",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "zhipu/glm-5.3" },
      modelCatalog: { "zhipu/glm-5.3": { alias: "GLM" } },
    },
  },
  {
    name: "Zhipu GLM en",
    family: "zhipu",
    regionKey: "intl",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe?ic=8JVLJQFSKB",
    settingsConfig: {
      baseUrl: "https://api.z.ai/api/coding/paas/v4",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "glm-5.3",
          name: "GLM-5.3",
          contextWindow: 1048576,
          cost: { input: 1.4, output: 4.4, cacheRead: 0.26 },
        },
        {
          id: "glm-5.3-flash",
          name: "GLM-5.3-Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 0.15, output: 0.5, cacheRead: 0.03 },
        },
      ],
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.z.ai/api/coding/paas/v4",
        defaultValue: "https://api.z.ai/api/coding/paas/v4",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "zhipu-en/glm-5.3" },
      modelCatalog: { "zhipu-en/glm-5.3": { alias: "GLM" } },
    },
  },
  {
    // 腾讯云 Token Plan 个人版（1823/130060，2026-08-21 版）：通用 + Hy 两
    // 系列共用同一端点与 API Key；Auto 智能路由调用 ID 是 tc-code-latest。
    // 模型条目照官方 OpenClaw 接入页（1823/130062，2026-08-27 版）原样：
    // cost 全零、ctx/maxTokens 为官方 OpenClaw 口径（≠平台模型列表页，如
    // deepseek 1000000≠1048576，勿按平台口径"修正"）。kimi-k2.5 官方
    // 2026-08-31 下线不收（接入页仍列，随阵容口径弃用）。超出接入页的
    // minimax-m2.7/glm-5.1/glm-5.2/hy3 按平台模型列表页（1300/78934）补
    // maxTokens；hy3 reasoning:true 与 hy3-preview 一致（Preserved Thinking）
    name: "Tencent Token Plan",
    family: "tencent",
    planKey: "tokenPlan",
    regionKey: "cn",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan",
    settingsConfig: {
      baseUrl: "https://api.lkeap.cloud.tencent.com/plan/v3",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "tc-code-latest",
          name: "Auto",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 196608,
          maxTokens: 32768,
        },
        {
          id: "deepseek-v4-flash-202605",
          name: "DeepSeek V4 Flash",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1000000,
          maxTokens: 384000,
        },
        {
          id: "deepseek-v4-pro-202606",
          name: "DeepSeek V4 Pro",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1000000,
          maxTokens: 384000,
        },
        // 接入页未列：maxTokens 按平台列表（128k）；reasoning 同族接入页口径
        {
          id: "minimax-m2.7",
          name: "MiniMax M2.7",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 200000,
          maxTokens: 131072,
        },
        {
          id: "glm-5",
          name: "GLM-5",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 202752,
          maxTokens: 16384,
        },
        // 接入页未列：maxTokens 按平台列表（128k）；reasoning 同族接入页口径
        {
          id: "glm-5.1",
          name: "GLM-5.1",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 200000,
          maxTokens: 131072,
        },
        // 接入页未列：maxTokens 按平台列表（128k，与企业版接入页 131072 一致）
        {
          id: "glm-5.2",
          name: "GLM-5.2",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 131072,
        },
        // 接入页未列：hy3 与 hy3-preview 同模型（preview 调用自动路由至
        // hy3），reasoning/口径随接入页 hy3-preview；maxTokens 按平台列表
        {
          id: "hy3",
          name: "Hy3",
          reasoning: true,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 256000,
          maxTokens: 131072,
        },
        {
          id: "hy3-preview",
          name: "Hy3 Preview",
          reasoning: true,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 262144,
          maxTokens: 131072,
        },
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.lkeap.cloud.tencent.com/plan/v3",
        defaultValue: "https://api.lkeap.cloud.tencent.com/plan/v3",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "tencent-tokenplan/tc-code-latest" },
      modelCatalog: {
        "tencent-tokenplan/tc-code-latest": { alias: "Auto" },
      },
    },
  },
  {
    // 国际站（新加坡地域）个人版（intl 1300/81315，2026-08-20 版）：Auto
    // 调用 ID 是 auto（≠国内 tc-code-latest），阵容不同（无 GLM-5/5.1/
    // Hy3）。端点用国际站文档钦定的 tencentcloudmaas.com 域；Key 按站独立。
    // 个人版无国际站专属 OpenClaw 接入页：auto/glm-5.2/minimax-m3 取自
    // 企业版接入页（1300/81503）口径、deepseek 取自国内个人版接入页
    //（1823/130062）口径，cost 全零；kimi-k2.6 接入页未列，maxTokens 按
    // 平台模型列表页（1300/78934，256k）
    name: "Tencent Token Plan (Intl)",
    family: "tencent",
    planKey: "tokenPlan",
    regionKey: "intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan",
    settingsConfig: {
      baseUrl: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "auto",
          name: "Auto",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 196608,
          maxTokens: 32768,
        },
        {
          id: "glm-5.2",
          name: "GLM-5.2",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 131072,
        },
        // 接入页未列：maxTokens 按平台列表（256k）；reasoning 同族接入页口径
        {
          id: "kimi-k2.6",
          name: "Kimi K2.6",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 262144,
          maxTokens: 262144,
        },
        {
          id: "deepseek-v4-pro-202606",
          name: "DeepSeek V4 Pro",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1000000,
          maxTokens: 384000,
        },
        {
          id: "deepseek-v4-flash-202605",
          name: "DeepSeek V4 Flash",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1000000,
          maxTokens: 384000,
        },
        {
          id: "minimax-m3",
          name: "MiniMax M3",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 32768,
        },
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
        defaultValue: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "tencent-tokenplan-intl/auto" },
      modelCatalog: {
        "tencent-tokenplan-intl/auto": { alias: "Auto" },
      },
    },
  },
  {
    // Token Plan 企业版专业套餐（1823/130659，2026-08-25 版，广州地域）：
    // 模型条目照官方企业版 OpenClaw 接入页（1300/81503，Pro 块）原样
    //（cost 全零、ctx/maxTokens 为官方 OpenClaw 口径）；glm-5/deepseek
    // 带日期对取个人版接入页（1823/130062）口径；接入页未列的
    // glm-5.3/glm-5.1/glm-5-turbo/kimi-k2.6/minimax-m2.7/deepseek-*-0731/
    // -0813 按平台模型列表页（1300/78934）补 maxTokens、reasoning 随同族
    // 接入页口径（全 false）。kimi-k2.5 官方 2026-08-31 下线不收；
    // minimax-m2.5 官方已除名且平台计划下线，2026-09-07 从全部 app 移除
    name: "Tencent Token Plan Enterprise Pro",
    family: "tencent",
    planKey: "enterprisePro",
    regionKey: "cn",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan-e",
    settingsConfig: {
      baseUrl: "https://tokenhub.tencentmaas.com/plan/v3",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "auto",
          name: "Auto",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 196608,
          maxTokens: 32768,
        },
        // 接入页未列：maxTokens 按平台列表（128k）
        {
          id: "glm-5.3",
          name: "GLM-5.3",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 131072,
        },
        {
          id: "glm-5.2",
          name: "GLM-5.2",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 131072,
        },
        {
          id: "glm-5",
          name: "GLM-5",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 202752,
          maxTokens: 16384,
        },
        // 接入页未列：maxTokens 按平台列表（128k）
        {
          id: "glm-5.1",
          name: "GLM-5.1",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 200000,
          maxTokens: 131072,
        },
        // 接入页未列：maxTokens 按平台列表（128k）
        {
          id: "glm-5-turbo",
          name: "GLM-5 Turbo",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 200000,
          maxTokens: 131072,
        },
        {
          id: "kimi-k2.7-code",
          name: "Kimi K2.7 Code",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 262144,
          maxTokens: 262144,
        },
        {
          id: "kimi-k2.7-code-highspeed",
          name: "Kimi K2.7 Code HighSpeed",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 262144,
          maxTokens: 262144,
        },
        // 接入页未列：maxTokens 按平台列表（256k）
        {
          id: "kimi-k2.6",
          name: "Kimi K2.6",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 262144,
          maxTokens: 262144,
        },
        // 接入页未列：maxTokens 按平台列表（128k）
        {
          id: "minimax-m2.7",
          name: "MiniMax M2.7",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 200000,
          maxTokens: 131072,
        },
        {
          id: "minimax-m3",
          name: "MiniMax M3",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 32768,
        },
        {
          id: "deepseek-v4-flash",
          name: "DeepSeek V4 Flash",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 393216,
        },
        {
          id: "deepseek-v4-pro",
          name: "DeepSeek V4 Pro",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 393216,
        },
        // 接入页未列：maxTokens 按平台列表（384k，与企业版接入页同口径）
        {
          id: "deepseek-v4-flash-0731",
          name: "DeepSeek V4 Flash 0731 GA",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 393216,
        },
        // 接入页未列：maxTokens 按平台列表（384k，与企业版接入页同口径）
        {
          id: "deepseek-v4-pro-0813",
          name: "DeepSeek V4 Pro 0813 GA",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 393216,
        },
        {
          id: "deepseek-v4-flash-202605",
          name: "DeepSeek V4 Flash Official",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 393216,
        },
        {
          id: "deepseek-v4-pro-202606",
          name: "DeepSeek V4 Pro Official",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 393216,
        },
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://tokenhub.tencentmaas.com/plan/v3",
        defaultValue: "https://tokenhub.tencentmaas.com/plan/v3",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "tencent-tokenplan-epro/auto" },
      modelCatalog: {
        "tencent-tokenplan-epro/auto": { alias: "Auto" },
      },
    },
  },
  {
    // 国际站企业版专业套餐（intl 1300/81489，2026-08-26 版，新加坡地域）：
    // 阵容为广州地域子集（无 GLM-5/5.1/5-Turbo、Kimi-K2.6、MiniMax-M2.7）。
    // 模型条目照官方企业版 OpenClaw 接入页（1300/81503，Pro 块）原样
    //（cost 全零）；接入页未列的 glm-5.3/deepseek-*-0731/-0813 按平台
    // 模型列表页（1300/78934）补 maxTokens、reasoning 随同族口径
    name: "Tencent Token Plan Enterprise Pro (Intl)",
    family: "tencent",
    planKey: "enterprisePro",
    regionKey: "intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan-e",
    settingsConfig: {
      baseUrl: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "auto",
          name: "Auto",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 196608,
          maxTokens: 32768,
        },
        // 接入页未列：maxTokens 按平台列表（128k）
        {
          id: "glm-5.3",
          name: "GLM-5.3",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 131072,
        },
        {
          id: "glm-5.2",
          name: "GLM-5.2",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 131072,
        },
        {
          id: "minimax-m3",
          name: "MiniMax M3",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 32768,
        },
        {
          id: "kimi-k2.7-code",
          name: "Kimi K2.7 Code",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 262144,
          maxTokens: 262144,
        },
        {
          id: "kimi-k2.7-code-highspeed",
          name: "Kimi K2.7 Code HighSpeed",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 262144,
          maxTokens: 262144,
        },
        {
          id: "deepseek-v4-flash",
          name: "DeepSeek V4 Flash",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 393216,
        },
        {
          id: "deepseek-v4-pro",
          name: "DeepSeek V4 Pro",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 393216,
        },
        // 接入页未列：maxTokens 按平台列表（384k，与企业版接入页同口径）
        {
          id: "deepseek-v4-flash-0731",
          name: "DeepSeek V4 Flash 0731 GA",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 393216,
        },
        // 接入页未列：maxTokens 按平台列表（384k，与企业版接入页同口径）
        {
          id: "deepseek-v4-pro-0813",
          name: "DeepSeek V4 Pro 0813 GA",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 393216,
        },
        {
          id: "deepseek-v4-flash-202605",
          name: "DeepSeek V4 Flash Official",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 393216,
        },
        {
          id: "deepseek-v4-pro-202606",
          name: "DeepSeek V4 Pro Official",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 1048576,
          maxTokens: 393216,
        },
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
        defaultValue: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "tencent-tokenplan-epro-intl/auto" },
      modelCatalog: {
        "tencent-tokenplan-epro-intl/auto": { alias: "Auto" },
      },
    },
  },
  {
    // Token Plan 企业版轻享套餐（1823/131173，2026-08-28 版）：仅 Auto 模型。
    // 条目照官方企业版 OpenClaw 接入页（1300/81503，Lite 块）原样
    name: "Tencent Token Plan Enterprise Lite",
    family: "tencent",
    planKey: "enterpriseLite",
    regionKey: "cn",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan-e",
    settingsConfig: {
      baseUrl: "https://tokenhub.tencentmaas.com/plan/v3",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "auto",
          name: "Auto",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 196608,
          maxTokens: 32768,
        },
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://tokenhub.tencentmaas.com/plan/v3",
        defaultValue: "https://tokenhub.tencentmaas.com/plan/v3",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "tencent-tokenplan-elite/auto" },
      modelCatalog: {
        "tencent-tokenplan-elite/auto": { alias: "Auto" },
      },
    },
  },
  {
    // 国际站企业版轻享套餐（intl 1300/81490）：新加坡地域（资源调度范围
    // Global），仅 Auto 模型。条目照官方企业版 OpenClaw 接入页
    //（1300/81503，Lite 块）原样
    name: "Tencent Token Plan Enterprise Lite (Intl)",
    family: "tencent",
    planKey: "enterpriseLite",
    regionKey: "intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan-e",
    settingsConfig: {
      baseUrl: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "auto",
          name: "Auto",
          reasoning: false,
          input: ["text"],
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 196608,
          maxTokens: 32768,
        },
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
        defaultValue: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "tencent-tokenplan-elite-intl/auto" },
      modelCatalog: {
        "tencent-tokenplan-elite-intl/auto": { alias: "Auto" },
      },
    },
  },
  {
    // 千帆 Token Plan 个人版（2026-07-13 起替代 Coding Plan 发售）。模型
    // 条目照官方 OpenClaw 接入页（2026-07-22 版）原样：cost/窗口 98304/
    // maxTokens 65536 均为官方钦定的 OpenClaw 口径（≠平台模型列表页 1M，
    // 与智谱预设 128000≠平台 200K 同款惯例，勿按平台口径"修正"）
    name: "Baidu Qianfan Token Plan",
    websiteUrl: "https://cloud.baidu.com/product/codingplan.html",
    apiKeyUrl: "https://console.bce.baidu.com/qianfan/resource/token-plan",
    settingsConfig: {
      baseUrl: "https://qianfan.baidubce.com/v2/tokenplan/personal",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "deepseek-v4-pro",
          name: "deepseek-v4-pro",
          reasoning: false,
          input: ["text"],
          cost: { input: 0.0025, output: 0.01, cacheRead: 0, cacheWrite: 0 },
          contextWindow: 98304,
          maxTokens: 65536,
        },
      ],
    },
    category: "cn_official",
    icon: "baidu",
    iconColor: "#2932E1",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://qianfan.baidubce.com/v2/tokenplan/personal",
        defaultValue: "https://qianfan.baidubce.com/v2/tokenplan/personal",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "qianfan-tokenplan/deepseek-v4-pro" },
      modelCatalog: {
        "qianfan-tokenplan/deepseek-v4-pro": { alias: "DeepSeek" },
      },
    },
  },
  {
    name: "千问AI平台",
    family: "qianwen",
    planKey: "payg",
    websiteUrl: "https://platform.qianwenai.com/?utm_content=g_20000002971",
    apiKeyUrl:
      "https://platform.qianwenai.com/home/api-keys?utm_content=g_20000002972",
    settingsConfig: {
      baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "qwen3.8-max",
          name: "Qwen3.8 Max",
          contextWindow: 983616,
        },
      ],
    },
    category: "cn_official",
    icon: "qianwenai",
    iconColor: "#624AFF",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        defaultValue: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "qwen/qwen3.8-max" },
      modelCatalog: { "qwen/qwen3.8-max": { alias: "Qwen" } },
    },
  },
  {
    name: "千问AI平台 Token Plan",
    family: "qianwen",
    planKey: "tokenPlan",
    websiteUrl:
      "https://platform.qianwenai.com/pricing/token-plan?utm_content=g_20000002977",
    apiKeyUrl:
      "https://platform.qianwenai.com/home/api-keys?utm_content=g_20000002978",
    settingsConfig: {
      baseUrl: "https://token-plan.cn-beijing.maas.aliyuncs.com/apps/anthropic",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "qwen3.8-max",
          name: "Qwen3.8 Max",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 983616,
          maxTokens: 131072,
        },
        {
          id: "qwen3.8-flash",
          name: "Qwen3.8 Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 983616,
          maxTokens: 131072,
        },
      ],
    },
    category: "cn_official",
    icon: "qianwenai",
    iconColor: "#624AFF",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder:
          "https://token-plan.cn-beijing.maas.aliyuncs.com/apps/anthropic",
        defaultValue:
          "https://token-plan.cn-beijing.maas.aliyuncs.com/apps/anthropic",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "qianwenai-token-plan/qwen3.8-max" },
      modelCatalog: {
        "qianwenai-token-plan/qwen3.8-max": { alias: "Qwen3.8 Max" },
        "qianwenai-token-plan/qwen3.8-flash": { alias: "Qwen3.8 Flash" },
      },
    },
  },
  // ===== QwenCloud（DashScope 国际站）=====
  // 与上面国内条目是两套独立站点：域名、控制台、密钥互不通用。
  // QwenCloud 与 Token Plan 都走 anthropic-messages，地址比 Claude Code 的
  // 多一段 /v1；国内 Token Plan 是唯一例外，官方文档给的就是不带 /v1 的
  // /apps/anthropic，勿照搬国际站补齐。
  // modelCatalog 必须逐条覆盖 models：OpenClaw 把 agents.defaults.models
  // 当白名单，漏写的模型会在客户端里被隐藏。
  {
    name: "QwenCloud",
    family: "qwencloud",
    planKey: "payg",
    websiteUrl: "https://home.qwencloud.com/?utm_content=g_20000002974",
    apiKeyUrl: "https://home.qwencloud.com/api-keys?utm_content=g_20000002975",
    settingsConfig: {
      baseUrl: "https://dashscope-intl.aliyuncs.com/apps/anthropic/v1",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "qwen3.8-max",
          name: "Qwen3.8 Max",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 983616,
          maxTokens: 131072,
        },
        {
          id: "qwen3.8-flash",
          name: "Qwen3.8 Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 983616,
          maxTokens: 131072,
        },
        {
          id: "qwen3.7-max",
          name: "Qwen3.7 Max",
          input: ["text"],
          contextWindow: 1000000,
          maxTokens: 65536,
        },
      ],
    },
    category: "cn_official",
    icon: "qwencloud",
    iconColor: "#6336E7",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://dashscope-intl.aliyuncs.com/apps/anthropic/v1",
        defaultValue: "https://dashscope-intl.aliyuncs.com/apps/anthropic/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "qwencloud/qwen3.8-max" },
      modelCatalog: {
        "qwencloud/qwen3.8-max": { alias: "Qwen3.8 Max" },
        "qwencloud/qwen3.8-flash": { alias: "Qwen3.8 Flash" },
        "qwencloud/qwen3.7-max": { alias: "Qwen3.7 Max" },
      },
    },
  },
  {
    name: "QwenCloud For Coding",
    family: "qwencloud",
    planKey: "coding",
    websiteUrl: "https://www.qwencloud.com",
    apiKeyUrl: "https://home.qwencloud.com/api-keys",
    settingsConfig: {
      baseUrl: "https://coding-intl.dashscope.aliyuncs.com/apps/anthropic/v1",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "qwen3.7-plus",
          name: "Qwen3.7 Plus",
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 65536,
        },
        {
          id: "qwen3.6-plus",
          name: "Qwen3.6 Plus",
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 65536,
        },
        {
          id: "qwen3-coder-plus",
          name: "Qwen3 Coder Plus",
          input: ["text"],
          contextWindow: 131072,
          maxTokens: 65536,
        },
      ],
    },
    category: "cn_official",
    icon: "qwencloud",
    iconColor: "#6336E7",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder:
          "https://coding-intl.dashscope.aliyuncs.com/apps/anthropic/v1",
        defaultValue:
          "https://coding-intl.dashscope.aliyuncs.com/apps/anthropic/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "qwencloud-coding/qwen3.7-plus" },
      modelCatalog: {
        "qwencloud-coding/qwen3.7-plus": { alias: "Qwen3.7 Plus" },
        "qwencloud-coding/qwen3.6-plus": { alias: "Qwen3.6 Plus" },
        "qwencloud-coding/qwen3-coder-plus": { alias: "Qwen3 Coder Plus" },
      },
    },
  },
  {
    name: "QwenCloud Token Plan",
    family: "qwencloud",
    planKey: "tokenPlan",
    websiteUrl:
      "https://www.qwencloud.com/pricing/token-plan?utm_content=g_20000002980",
    apiKeyUrl: "https://home.qwencloud.com/api-keys?utm_content=g_20000002981",
    settingsConfig: {
      baseUrl:
        "https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic/v1",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "qwen3.8-max",
          name: "Qwen3.8 Max",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 983616,
          maxTokens: 131072,
        },
        {
          id: "qwen3.8-flash",
          name: "Qwen3.8 Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 983616,
          maxTokens: 131072,
        },
        {
          id: "qwen3.7-max",
          name: "Qwen3.7 Max",
          input: ["text"],
          contextWindow: 1000000,
          maxTokens: 65536,
        },
      ],
    },
    category: "cn_official",
    icon: "qwencloud",
    iconColor: "#6336E7",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder:
          "https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic/v1",
        defaultValue:
          "https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "qwencloud-token-plan/qwen3.8-max" },
      modelCatalog: {
        "qwencloud-token-plan/qwen3.8-max": { alias: "Qwen3.8 Max" },
        "qwencloud-token-plan/qwen3.8-flash": { alias: "Qwen3.8 Flash" },
        "qwencloud-token-plan/qwen3.7-max": { alias: "Qwen3.7 Max" },
      },
    },
  },
  {
    name: "StepFun",
    family: "stepfun",
    regionKey: "cn",
    websiteUrl: "https://platform.stepfun.com/step-plan",
    apiKeyUrl: "https://platform.stepfun.com/interface-key",
    settingsConfig: {
      baseUrl: "https://api.stepfun.com/step_plan/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "step-3.5-flash-2603",
          name: "Step 3.5 Flash 2603",
          contextWindow: 262144,
        },
        {
          id: "step-3.5-flash",
          name: "Step 3.5 Flash",
          contextWindow: 262144,
        },
        {
          id: "step-3.7-flash",
          name: "Step 3.7 Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 256000,
          maxTokens: 256000,
          cost: { input: 0.19, output: 1.13, cacheRead: 0.04, cacheWrite: 0 },
        },
        {
          id: "step-5-preview",
          name: "Step 5 Preview",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 64000,
          cost: {
            input: 0.98,
            output: 2.8,
            cacheRead: 0.05,
            cacheWrite: 0,
          },
        },
      ],
    },
    category: "cn_official",
    icon: "stepfun",
    iconColor: "#16D6D2",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.stepfun.com/step_plan/v1",
        defaultValue: "https://api.stepfun.com/step_plan/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "step-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "stepfun/step-3.5-flash-2603" },
      modelCatalog: {
        "stepfun/step-3.5-flash-2603": { alias: "StepFun" },
        "stepfun/step-3.5-flash": { alias: "StepFun Flash" },
      },
    },
  },
  {
    name: "StepFun en",
    family: "stepfun",
    regionKey: "intl",
    websiteUrl: "https://platform.stepfun.ai/step-plan",
    apiKeyUrl: "https://platform.stepfun.ai/interface-key",
    settingsConfig: {
      baseUrl: "https://api.stepfun.ai/step_plan/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "step-3.5-flash-2603",
          name: "Step 3.5 Flash 2603",
          contextWindow: 262144,
        },
        {
          id: "step-3.5-flash",
          name: "Step 3.5 Flash",
          contextWindow: 262144,
        },
        {
          id: "step-3.7-flash",
          name: "Step 3.7 Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 256000,
          maxTokens: 256000,
          cost: { input: 0.19, output: 1.13, cacheRead: 0.04, cacheWrite: 0 },
        },
        {
          id: "step-5-preview",
          name: "Step 5 Preview",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 64000,
          cost: {
            input: 0.98,
            output: 2.8,
            cacheRead: 0.05,
            cacheWrite: 0,
          },
        },
      ],
    },
    category: "cn_official",
    icon: "stepfun",
    iconColor: "#16D6D2",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.stepfun.ai/step_plan/v1",
        defaultValue: "https://api.stepfun.ai/step_plan/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "step-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "stepfun-en/step-3.5-flash-2603" },
      modelCatalog: {
        "stepfun-en/step-3.5-flash-2603": { alias: "StepFun" },
        "stepfun-en/step-3.5-flash": { alias: "StepFun Flash" },
      },
    },
  },
  {
    name: "MiniMax",
    family: "minimax",
    regionKey: "cn",
    websiteUrl: "https://platform.minimax.cn",
    apiKeyUrl: "https://platform.minimax.cn/subscribe/token-plan",
    settingsConfig: {
      baseUrl: "https://api.minimax.cn/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "MiniMax-M3",
          name: "MiniMax M3",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 131072,
          cost: { input: 0.3, output: 1.2, cacheRead: 0.06, cacheWrite: 0 },
        },
      ],
    },
    category: "cn_official",
    theme: {
      backgroundColor: "#f64551",
      textColor: "#FFFFFF",
    },
    icon: "minimax",
    iconColor: "#FF6B6B",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "minimax/MiniMax-M3" },
      modelCatalog: { "minimax/MiniMax-M3": { alias: "MiniMax" } },
    },
  },
  {
    name: "MiniMax en",
    family: "minimax",
    regionKey: "intl",
    websiteUrl: "https://platform.minimax.io",
    apiKeyUrl: "https://platform.minimax.io/subscribe/coding-plan",
    settingsConfig: {
      baseUrl: "https://api.minimax.io/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "MiniMax-M3",
          name: "MiniMax M3",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 131072,
          cost: { input: 0.3, output: 1.2, cacheRead: 0.06, cacheWrite: 0 },
        },
      ],
    },
    category: "cn_official",
    theme: {
      backgroundColor: "#f64551",
      textColor: "#FFFFFF",
    },
    icon: "minimax",
    iconColor: "#FF6B6B",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "minimax-en/MiniMax-M3" },
      modelCatalog: { "minimax-en/MiniMax-M3": { alias: "MiniMax" } },
    },
  },
  {
    name: "StreamLake",
    family: "streamlake",
    regionKey: "cn",
    websiteUrl: "https://www.streamlake.com",
    apiKeyUrl: "https://console.streamlake.com/console/wanqing/api-key",
    settingsConfig: {
      baseUrl: "https://wanqing.streamlakeapi.com/api/gateway/v1/endpoints",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "kat-coder-pro-v2.5",
          name: "KAT-Coder Pro V2.5",
          contextWindow: 256000,
        },
      ],
    },
    category: "cn_official",
    icon: "streamlake",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
  },
  {
    name: "StreamLake en",
    family: "streamlake",
    regionKey: "intl",
    websiteUrl: "https://console.streamlake.ai",
    apiKeyUrl: "https://console.streamlake.ai/console/api-key",
    settingsConfig: {
      baseUrl: "https://vanchin.streamlake.ai/api/gateway/v1/endpoints",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "kat-coder-pro-v2.5",
          name: "KAT-Coder Pro V2.5",
          contextWindow: 256000,
        },
      ],
    },
    category: "cn_official",
    icon: "streamlake",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
  },
  {
    name: "Longcat",
    websiteUrl: "https://longcat.chat/platform",
    apiKeyUrl: "https://longcat.chat/platform/api_keys",
    settingsConfig: {
      baseUrl: "https://api.longcat.chat/openai/v1",
      apiKey: "",
      api: "openai-completions",
      authHeader: true,
      models: [
        {
          id: "LongCat-2.0",
          name: "LongCat 2.0",
          reasoning: false,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 131072,
          compat: { maxTokensField: "max_tokens" },
          cost: { input: 0.75, output: 2.95, cacheRead: 0.015 },
        },
      ],
    },
    category: "cn_official",
    icon: "longcat",
    iconColor: "#29E154",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.longcat.chat/openai/v1",
        defaultValue: "https://api.longcat.chat/openai/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "longcat/LongCat-2.0" },
      modelCatalog: { "longcat/LongCat-2.0": { alias: "LongCat" } },
    },
  },
  {
    name: "BaiLing",
    websiteUrl: "https://developer.ant-ling.com/zh-CN/docs/",
    apiKeyUrl: "https://chat.ant-ling.com/open",
    settingsConfig: {
      baseUrl: "https://api.ant-ling.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "Ling-2.6-1T",
          name: "Ling 2.6 1T",
          contextWindow: 262144,
          cost: { input: 0.63, output: 2.52 },
        },
      ],
    },
    category: "cn_official",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "bailing/Ling-2.6-1T" },
      modelCatalog: { "bailing/Ling-2.6-1T": { alias: "BaiLing" } },
    },
    icon: "bailing",
  },
  {
    name: "Xiaomi MiMo",
    family: "xiaomi-mimo",
    planKey: "payg",
    websiteUrl: "https://platform.xiaomimimo.com",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/api-keys",
    settingsConfig: {
      baseUrl: "https://api.xiaomimimo.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "mimo-v2.5-pro",
          name: "MiMo V2.5 Pro",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: {
            input: 0.435,
            output: 0.87,
            cacheRead: 0.0036,
            cacheWrite: 0,
          },
        },
        {
          id: "mimo-v2.6-pro",
          name: "MiMo V2.6 Pro",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 0.435, output: 0.87, cacheRead: 0.0036 },
        },
        {
          id: "mimo-v2.6-flash",
          name: "MiMo V2.6 Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 0.14, output: 0.28, cacheRead: 0.0028 },
        },
        {
          id: "mimo-v2.6-pro-ultraspeed",
          name: "MiMo V2.6 Pro UltraSpeed",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 4.35, output: 8.7, cacheRead: 0.036 },
        },
      ],
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
    suggestedDefaults: {
      model: { primary: "xiaomimimo/mimo-v2.6-pro" },
      modelCatalog: { "xiaomimimo/mimo-v2.6-pro": { alias: "MiMo" } },
    },
  },
  {
    name: "Xiaomi MiMo Token Plan (China)",
    family: "xiaomi-mimo",
    planKey: "tokenPlan",
    websiteUrl: "https://platform.xiaomimimo.com/#/token-plan",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/plan-manage",
    settingsConfig: {
      baseUrl: "https://token-plan-cn.xiaomimimo.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "mimo-v2.5-pro",
          name: "MiMo V2.5 Pro",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 131072,
        },
        {
          id: "mimo-v2.5",
          name: "MiMo V2.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
        },
        {
          id: "mimo-v2.6-pro",
          name: "MiMo V2.6 Pro",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 0.435, output: 0.87, cacheRead: 0.0036 },
        },
        {
          id: "mimo-v2.6-flash",
          name: "MiMo V2.6 Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 0.14, output: 0.28, cacheRead: 0.0028 },
        },
      ],
    },
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "Token Plan API Key",
        placeholder: "tp-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "xiaomi-mimo-token-plan/mimo-v2.6-pro" },
      modelCatalog: {
        "xiaomi-mimo-token-plan/mimo-v2.6-pro": {
          alias: "MiMo Token Plan (China)",
        },
        "xiaomi-mimo-token-plan/mimo-v2.5": {
          alias: "MiMo Token Plan (China) Multimodal",
        },
      },
    },
  },

  {
    name: "AiHubMix",
    websiteUrl: "https://aihubmix.com",
    apiKeyUrl: "https://aihubmix.com",
    settingsConfig: {
      baseUrl: "https://aihubmix.com",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 4, output: 20, cacheRead: 0.2, cacheWrite: 5 },
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 0.25, cacheWrite: 12.5 },
        },
      ],
    },
    category: "aggregator",
    icon: "aihubmix",
    iconColor: "#006FFB",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "aihubmix/claude-opus-5",
        fallbacks: ["aihubmix/claude-sonnet-5"],
      },
      modelCatalog: {
        "aihubmix/claude-opus-5": { alias: "Opus" },
        "aihubmix/claude-sonnet-5": { alias: "Sonnet" },
      },
    },
  },
  {
    name: "CherryIN",
    websiteUrl: "https://open.cherryin.ai",
    apiKeyUrl: "https://open.cherryin.ai/console/token",
    settingsConfig: {
      baseUrl: "https://open.cherryin.net",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "anthropic/claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
        },
        {
          id: "anthropic/claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
        },
        {
          id: "anthropic/claude-fable-5.1",
          name: "Claude Fable 5.1",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 0.25, cacheWrite: 12.5 },
        },
      ],
    },
    category: "aggregator",
    icon: "cherryin",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "cherryin/anthropic/claude-opus-5",
        fallbacks: ["cherryin/anthropic/claude-sonnet-5"],
      },
      modelCatalog: {
        "cherryin/anthropic/claude-opus-5": { alias: "Opus" },
        "cherryin/anthropic/claude-sonnet-5": { alias: "Sonnet" },
      },
    },
  },
  {
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      baseUrl: "https://openrouter.ai/api/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "anthropic/claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
        {
          id: "anthropic/claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: {
            input: 2,
            output: 10,
            cacheRead: 0.2,
            cacheWrite: 2.5,
          },
        },
        {
          id: "anthropic/claude-opus-5.5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: {
            input: 4,
            output: 20,
            cacheRead: 0.2,
            cacheWrite: 5,
          },
        },
        {
          id: "anthropic/claude-fable-5.1",
          name: "Claude Fable 5.1",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 0.25, cacheWrite: 12.5 },
        },
      ],
    },
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6566F1",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-or-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "openrouter/anthropic/claude-opus-5",
        fallbacks: ["openrouter/anthropic/claude-sonnet-5"],
      },
      modelCatalog: {
        "openrouter/anthropic/claude-opus-5": { alias: "Opus" },
        "openrouter/anthropic/claude-sonnet-5": { alias: "Sonnet" },
      },
    },
  },
  {
    name: "TheRouter",
    websiteUrl: "https://therouter.ai",
    apiKeyUrl: "https://dashboard.therouter.ai",
    settingsConfig: {
      baseUrl: "https://api.therouter.ai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "anthropic/claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
        {
          id: "openai/gpt-5.3-codex",
          name: "GPT-5.3 Codex",
          contextWindow: 400000,
          cost: { input: 5, output: 40, cacheRead: 0.5 },
        },
        {
          id: "openai/gpt-5.2",
          name: "GPT-5.2",
          contextWindow: 400000,
          cost: { input: 1.75, output: 14, cacheRead: 0.175 },
        },
        {
          id: "google/gemini-3.6-flash",
          name: "Gemini 3.6 Flash",
          contextWindow: 1000000,
          cost: { input: 1.5, output: 9, cacheRead: 0.15 },
        },
        {
          id: "qwen/qwen3-coder-480b",
          name: "Qwen3 Coder 480B",
          contextWindow: 262144,
          cost: { input: 0.6, output: 2.35 },
        },
      ],
    },
    category: "aggregator",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "therouter/anthropic/claude-sonnet-5",
        fallbacks: [
          "therouter/openai/gpt-5.2",
          "therouter/google/gemini-3.6-flash",
        ],
      },
      modelCatalog: {
        "therouter/anthropic/claude-sonnet-5": { alias: "Sonnet" },
        "therouter/openai/gpt-5.2": { alias: "GPT-5.2" },
        "therouter/google/gemini-3.6-flash": { alias: "Gemini Flash" },
        "therouter/openai/gpt-5.3-codex": { alias: "Codex" },
        "therouter/qwen/qwen3-coder-480b": { alias: "Qwen Coder" },
      },
    },
    icon: "therouter",
  },
  {
    name: "ModelScope",
    websiteUrl: "https://modelscope.cn",
    apiKeyUrl: "https://modelscope.cn/my/myaccesstoken",
    settingsConfig: {
      baseUrl: "https://api-inference.modelscope.cn/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "ZhipuAI/GLM-5.2",
          name: "GLM-5.2",
          contextWindow: 128000,
          cost: { input: 1.4, output: 4.4, cacheRead: 0.26 },
        },
      ],
    },
    category: "aggregator",
    icon: "modelscope",
    iconColor: "#624AFF",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api-inference.modelscope.cn/v1",
        defaultValue: "https://api-inference.modelscope.cn/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "modelscope/ZhipuAI/GLM-5.2" },
      modelCatalog: { "modelscope/ZhipuAI/GLM-5.2": { alias: "GLM" } },
    },
  },
  {
    name: "Baseten",
    websiteUrl: "https://www.baseten.co",
    apiKeyUrl: "https://app.baseten.co/settings/api_keys",
    // OpenAI Chat 端点 /v1/chat/completions（docs.baseten.co/development/
    // model-apis/overview，2026-10-14 核对）；窗口、最大输出与输入模态取同页
    // 模型表
    settingsConfig: {
      baseUrl: "https://inference.baseten.co/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "zai-org/GLM-5.3",
          name: "GLM-5.3",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 262144,
        },
        {
          id: "moonshotai/Kimi-K3",
          name: "Kimi K3",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 262144,
        },
        {
          id: "zai-org/GLM-5.3-Flash",
          name: "GLM-5.3 Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 262144,
        },
        {
          id: "deepseek-ai/DeepSeek-V4.1-Flash",
          name: "DeepSeek V4.1 Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 262144,
        },
      ],
    },
    category: "aggregator",
    icon: "baseten",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "baseten/zai-org/GLM-5.3" },
      modelCatalog: { "baseten/zai-org/GLM-5.3": { alias: "GLM" } },
    },
  },
  {
    name: "Novita AI",
    websiteUrl: "https://novita.ai",
    apiKeyUrl: "https://novita.ai",
    settingsConfig: {
      baseUrl: "https://api.novita.ai/openai",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "zai-org/glm-5.1",
          name: "GLM-5.1",
          contextWindow: 202800,
          cost: { input: 1.38, output: 4.4, cacheRead: 0.26, cacheWrite: 0 },
        },
        {
          id: "zai-org/glm-5.3",
          name: "GLM-5.3",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 1.4, output: 4.4, cacheRead: 0.26, cacheWrite: 0 },
        },
        {
          id: "zai-org/glm-5.3-flash",
          name: "GLM-5.3-Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 0.15, output: 0.5, cacheRead: 0.03, cacheWrite: 0 },
        },
        {
          id: "moonshotai/kimi-k3",
          name: "Kimi K3",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 0 },
        },
      ],
    },
    category: "aggregator",
    icon: "novita",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "novita/zai-org/glm-5.1" },
      modelCatalog: {
        "novita/zai-org/glm-5.1": { alias: "GLM-5.1" },
      },
    },
  },
  {
    name: "Nvidia",
    websiteUrl: "https://build.nvidia.com",
    apiKeyUrl: "https://build.nvidia.com/settings/api-keys",
    settingsConfig: {
      baseUrl: "https://integrate.api.nvidia.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "moonshotai/kimi-k3",
          name: "Kimi K3",
          contextWindow: 1048576,
          maxTokens: 131072,
          input: ["text", "image"],
          reasoning: true,
        },
        {
          id: "z-ai/glm-5.3",
          name: "GLM-5.3",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 131072,
        },
        {
          id: "z-ai/glm-5.3-flash",
          name: "GLM-5.3-Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
        },
      ],
    },
    category: "aggregator",
    icon: "nvidia",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "nvapi-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "nvidia/moonshotai/kimi-k3" },
      modelCatalog: { "nvidia/moonshotai/kimi-k3": { alias: "Kimi" } },
    },
  },
  {
    name: "Ollama Cloud",
    websiteUrl: "https://ollama.com",
    apiKeyUrl: "https://ollama.com/settings/keys",
    // 云端 OpenAI 兼容 base 为 https://ollama.com/v1（docs.ollama.com/api/
    // openai-compatibility，2026-10-14 核对）。cost 取 https://ollama.com/pricing
    // 同日标准时段价（非 off-peak 半价）；maxTokens 平台未公布，取 models.dev
    // 的 ollama-cloud 条目，DeepSeek 两款按官方 384K
    settingsConfig: {
      baseUrl: "https://ollama.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "glm-5.3",
          name: "GLM-5.3",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 1.4, output: 4.4, cacheRead: 0.26, cacheWrite: 0 },
        },
        {
          id: "glm-5.3-flash",
          name: "GLM-5.3-Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 0.15, output: 0.5, cacheRead: 0.03, cacheWrite: 0 },
        },
        {
          id: "kimi-k3",
          name: "Kimi K3",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 0 },
        },
        {
          id: "deepseek-v4-pro:0813",
          name: "DeepSeek V4 Pro",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 384000,
          cost: { input: 1.32, output: 3.96, cacheRead: 0.044, cacheWrite: 0 },
        },
        {
          id: "deepseek-v4.1-flash",
          name: "DeepSeek V4.1 Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 384000,
          cost: { input: 0.3, output: 1.2, cacheRead: 0.006, cacheWrite: 0 },
        },
        {
          id: "minimax-m3",
          name: "MiniMax M3",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 512000,
          maxTokens: 131072,
          cost: { input: 0.6, output: 2.4, cacheRead: 0.12, cacheWrite: 0 },
        },
      ],
    },
    category: "cloud_provider",
    icon: "ollama",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "ollama-cloud/glm-5.3" },
      modelCatalog: {
        "ollama-cloud/glm-5.3": { alias: "GLM-5.3" },
      },
    },
  },
  {
    name: "LM Studio",
    websiteUrl: "https://lmstudio.ai",
    // 本机 OpenAI Chat base http://localhost:1234/v1（docs.openclaw.ai/providers/
    // lmstudio，2026-10-14 核对）。OpenClaw 自带 lmstudio 供应商，供应商名请
    // 另取（下面示例用 lmstudio-local）。默认不鉴权，未开鉴权时 OpenClaw 也要
    // 存一个非机密占位值，预填官方占位 Key lmstudio；开启鉴权后换成 API Token。
    // 本地推理不计费，cost 记 0；maxTokens 取 models.dev 的 lmstudio 条目。
    // 模型换成自己已下载的；实际窗口受 LM Studio 加载模型时的 context length
    // 限制（UI 设置或 lms load --context-length），官方要求至少约 25K，加载
    // 窗口更小时把 contextWindow 一起调小
    settingsConfig: {
      baseUrl: "http://localhost:1234/v1",
      apiKey: "lmstudio",
      api: "openai-completions",
      models: [
        {
          id: "openai/gpt-oss-20b",
          name: "GPT-OSS 20B",
          reasoning: true,
          input: ["text"],
          contextWindow: 131072,
          maxTokens: 32768,
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
        },
      ],
    },
    category: "local",
    icon: "lmstudio",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "lmstudio",
        editorValue: "lmstudio",
      },
    },
    suggestedDefaults: {
      model: { primary: "lmstudio-local/openai/gpt-oss-20b" },
      modelCatalog: {
        "lmstudio-local/openai/gpt-oss-20b": { alias: "GPT-OSS 20B" },
      },
    },
  },
  {
    name: "llama.cpp",
    websiteUrl: "https://github.com/ggml-org/llama.cpp",
    // 本机 llama-server 的 OpenAI Chat base http://127.0.0.1:9931/v1（github.com/
    // openclaw/openclaw/blob/main/docs/gateway/config-tools/provider-examples.md、
    // docs/plugins/llama-cpp.md，2026-10-14 核对）。OpenClaw 自带 llama-cpp 插件
    // 供应商，供应商名请另取（下面示例用 llamacpp-local）；自定义 ID 指向
    // llama-server 时官方要求每个模型声明 compat.toolSchemaProfile=llamacpp，
    // 清掉 GBNF 编译不了的 pattern / maxLength（影响 cron 工具）。默认端口
    // b11521（PR #30159）起为 9931，v0.6.0 及更早、Homebrew 与 Docker 仍是 8080。
    // 不带 --api-key 时不校验，预填占位值 no-key（官方示例用 llamacpp-no-key，
    // 任意非空值都行）。本地推理不计费，cost 记 0。默认模型取 ggml-org 官方
    // GGUF 的 Qwen3-Coder 30B A3B（窗口 / 输出取 Qwen 模型卡），换成自己已下载
    // 的；本地实际窗口受 -c / --fit / -np 限制，更小时把 contextWindow 一起调小
    settingsConfig: {
      baseUrl: "http://127.0.0.1:9931/v1",
      apiKey: "no-key",
      api: "openai-completions",
      models: [
        {
          id: "ggml-org/Qwen3-Coder-30B-A3B-Instruct-Q8_0-GGUF",
          name: "Qwen3-Coder 30B A3B",
          reasoning: false,
          input: ["text"],
          contextWindow: 262144,
          maxTokens: 65536,
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
          compat: { supportsTools: true, toolSchemaProfile: "llamacpp" },
        },
      ],
    },
    category: "local",
    icon: "llamacpp",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "no-key",
        editorValue: "no-key",
      },
    },
    suggestedDefaults: {
      model: {
        primary:
          "llamacpp-local/ggml-org/Qwen3-Coder-30B-A3B-Instruct-Q8_0-GGUF",
      },
      modelCatalog: {
        "llamacpp-local/ggml-org/Qwen3-Coder-30B-A3B-Instruct-Q8_0-GGUF": {
          alias: "Qwen3-Coder 30B A3B",
        },
      },
    },
  },
  {
    name: "vLLM",
    websiteUrl: "https://vllm.ai",
    // 本机 vllm serve 的 OpenAI Chat base http://localhost:8000/v1（docs.openclaw.ai/
    // providers/vllm、docs.vllm.ai/en/latest/serving/online_serving/
    // openai_compatible_server，2026-10-14 核对）。OpenClaw 自带 vllm 供应商
    //（设了 VLLM_API_KEY 时自动发现模型），供应商名请另取（下面示例用
    // vllm-local）。默认端口 8000，不带 --host 时监听全部 IPv4 网卡，建议加
    // --host 127.0.0.1；工具调用要启动时加 --enable-auto-tool-choice
    // --tool-call-parser qwen3_coder，思考另加 --reasoning-parser qwen3。不带
    // --api-key 时不校验，预填官方占位值 dummy（任意非空值都行，带了就填那个值）。
    // 本地推理不计费，cost 记 0。默认模型取 vLLM Codex 接入页的示例
    // Qwen/Qwen3.6-27B（窗口 / 输出取 Qwen 模型卡），须与 /v1/models 的 id 一致，
    // 换成自己已下载的；本地实际窗口受 --max-model-len 限制，更小时把
    // contextWindow 一起调小
    settingsConfig: {
      baseUrl: "http://localhost:8000/v1",
      apiKey: "dummy",
      api: "openai-completions",
      models: [
        {
          id: "Qwen/Qwen3.6-27B",
          name: "Qwen3.6 27B",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 262144,
          maxTokens: 32768,
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
        },
      ],
    },
    category: "local",
    icon: "vllm",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "dummy",
        editorValue: "dummy",
      },
    },
    suggestedDefaults: {
      model: { primary: "vllm-local/Qwen/Qwen3.6-27B" },
      modelCatalog: {
        "vllm-local/Qwen/Qwen3.6-27B": { alias: "Qwen3.6 27B" },
      },
    },
  },
  {
    name: "Meta Model API",
    websiteUrl: "https://dev.meta.ai",
    apiKeyUrl: "https://dev.meta.ai",
    // OpenAI Chat 端点 /v1/chat/completions（dev.meta.ai/docs/coding-agents.md，
    // 2026-10-14 核对），Bearer 鉴权。cost 取 dev.meta.ai/docs/
    // pricing-rate-limits.md 同日 Standard mode 价（每百万 token）；缓存自动
    // 生效、写入不另收故记 0。最大输出取官方 OpenCode 配置的 limit.output
    settingsConfig: {
      baseUrl: "https://api.meta.ai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "muse-wonder-1.0",
          name: "Muse Wonder 1.0",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 4, output: 20, cacheRead: 0.2, cacheWrite: 0 },
        },
        {
          id: "muse-spark-1.3",
          name: "Muse Spark 1.3",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 1.25, output: 4.25, cacheRead: 0.15, cacheWrite: 0 },
        },
      ],
    },
    category: "cn_official",
    icon: "meta",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "meta/muse-wonder-1.0" },
      modelCatalog: {
        "meta/muse-wonder-1.0": { alias: "Muse Wonder" },
      },
    },
  },
  {
    name: "DeepInfra",
    websiteUrl: "https://deepinfra.com",
    apiKeyUrl: "https://deepinfra.com/dash/api_keys",
    // OpenAI Chat 端点 base 为 /v1/openai（docs.deepinfra.com/chat/overview.md，
    // 2026-10-14 核对），Bearer 鉴权。cost 取 https://api.deepinfra.com/models/<id>
    // 同日原价（每百万 token）：GLM-5.2 / 5.3 网页另有无截止日期的折扣，促销价
    // 会变，故记原价；缓存写入不另收记 0。窗口与最大输出同 OpenCode 预设
    settingsConfig: {
      baseUrl: "https://api.deepinfra.com/v1/openai",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "deepseek-ai/DeepSeek-V4-Flash-0731",
          name: "DeepSeek V4 Flash 0731",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 384000,
          cost: { input: 0.06, output: 0.18, cacheRead: 0.015, cacheWrite: 0 },
        },
        {
          id: "deepseek-ai/DeepSeek-V4-Pro-0813",
          name: "DeepSeek V4 Pro 0813",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 384000,
          cost: { input: 1.3, output: 2.6, cacheRead: 0.1, cacheWrite: 0 },
        },
        {
          id: "moonshotai/Kimi-K3",
          name: "Kimi K3",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 2.85, output: 14.25, cacheRead: 0.285, cacheWrite: 0 },
        },
        {
          id: "zai-org/GLM-5.3",
          name: "GLM-5.3",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 0.9, output: 4, cacheRead: 0.2, cacheWrite: 0 },
        },
        {
          id: "zai-org/GLM-5.2",
          name: "GLM-5.2",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 163840,
          cost: { input: 0.75, output: 2.4, cacheRead: 0.14, cacheWrite: 0 },
        },
      ],
    },
    category: "aggregator",
    icon: "deepinfra",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "deepinfra/deepseek-ai/DeepSeek-V4-Flash-0731" },
      modelCatalog: {
        "deepinfra/deepseek-ai/DeepSeek-V4-Flash-0731": {
          alias: "DeepSeek V4 Flash 0731",
        },
      },
    },
  },
  {
    name: "Vercel AI Gateway",
    websiteUrl: "https://vercel.com/ai-gateway",
    apiKeyUrl:
      "https://vercel.com/d?to=%2F%5Bteam%5D%2F%7E%2Fai-gateway%2Fapi-keys",
    // 官方 OpenClaw 接入文档（vercel.com/docs/ai-gateway/coding-agents/openclaw，
    // 2026-10-14 核对）原文即 /coding-agent/v1 + openai-completions，且只有写进
    // models 的模型才能路由。cost 取 GET https://ai-gateway.vercel.sh/v1/models
    // 同日公开价（每百万 token）的低档价：Haiku 5.5 超 100K、GPT-6 系超 272K
    // 后单价上浮；Gemini 3.8 Flash 未列缓存写入记 0。窗口与最大输出同 OpenCode
    // 预设
    settingsConfig: {
      baseUrl: "https://ai-gateway.vercel.sh/coding-agent/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "anthropic/claude-opus-5.5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 4, output: 20, cacheRead: 0.2, cacheWrite: 5 },
        },
        {
          id: "anthropic/claude-sonnet-5.5",
          name: "Claude Sonnet 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 2, output: 10, cacheRead: 0.1, cacheWrite: 2.5 },
        },
        {
          id: "anthropic/claude-haiku-5.5",
          name: "Claude Haiku 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 0.1, output: 0.5, cacheRead: 0.01, cacheWrite: 0.125 },
        },
        {
          id: "openai/gpt-6-astra",
          name: "GPT-6 Astra",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 1, cacheWrite: 12.5 },
        },
        {
          id: "openai/gpt-6.1-sol",
          name: "GPT-6.1 Sol",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 2, output: 10, cacheRead: 0.1, cacheWrite: 2.5 },
        },
        {
          id: "google/gemini-3.8-flash",
          name: "Gemini 3.8 Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 65535,
          cost: { input: 0.75, output: 3.75, cacheRead: 0.075, cacheWrite: 0 },
        },
      ],
    },
    category: "aggregator",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "vercel-ai-gateway/anthropic/claude-opus-5.5",
        fallbacks: ["vercel-ai-gateway/anthropic/claude-sonnet-5.5"],
      },
      modelCatalog: {
        "vercel-ai-gateway/anthropic/claude-opus-5.5": { alias: "Opus" },
        "vercel-ai-gateway/anthropic/claude-sonnet-5.5": { alias: "Sonnet" },
      },
    },
    icon: "vercel",
  },
  {
    name: "Nebius Token Factory",
    websiteUrl: "https://nebius.com/token-factory",
    apiKeyUrl: "https://tokenfactory.nebius.com/project/api-keys",
    // OpenAI Chat 端点 base 为 /v1（docs.tokenfactory.nebius.com/switch.md，
    // 2026-10-14 核对），Bearer 鉴权。cost 取官方公开目录
    // tokenfactory.nebius.com/api/public/models_info 同日价（每百万 token）；
    // 目录里所有模型都没有缓存价，故只写 input / output。窗口与最大输出同
    // OpenCode 预设
    settingsConfig: {
      baseUrl: "https://api.tokenfactory.nebius.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "zai-org/GLM-5.3",
          name: "GLM-5.3",
          reasoning: true,
          input: ["text"],
          contextWindow: 1024000,
          maxTokens: 131072,
          cost: { input: 1.4, output: 4.4 },
        },
        {
          id: "zai-org/GLM-5.3-Flash",
          name: "GLM-5.3-Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1024000,
          maxTokens: 131072,
          cost: { input: 0.15, output: 0.5 },
        },
        {
          id: "moonshotai/Kimi-K3",
          name: "Kimi K3",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1024000,
          maxTokens: 131072,
          cost: { input: 3, output: 15 },
        },
        {
          id: "moonshotai/Kimi-K2.7-Code",
          name: "Kimi K2.7 Code",
          reasoning: true,
          input: ["text"],
          contextWindow: 262144,
          maxTokens: 262144,
          cost: { input: 0.95, output: 4 },
        },
        {
          id: "deepseek-ai/DeepSeek-V4-Pro-0813",
          name: "DeepSeek V4 Pro 0813",
          reasoning: true,
          input: ["text"],
          contextWindow: 979000,
          maxTokens: 384000,
          cost: { input: 1.32, output: 3.96 },
        },
      ],
    },
    category: "aggregator",
    icon: "nebius",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "nebius/zai-org/GLM-5.3" },
      modelCatalog: {
        "nebius/zai-org/GLM-5.3": { alias: "GLM-5.3" },
      },
    },
  },
  {
    name: "Groq",
    websiteUrl: "https://groq.com",
    apiKeyUrl: "https://console.groq.com/keys",
    // OpenAI Chat 端点 base 为 https://api.groq.com/openai/v1（console.groq.com/
    // docs/openai，2026-10-14 核对），Bearer 鉴权。cost 取各模型页同日价
    //（console.groq.com/docs/model/openai/gpt-oss-120b、.../gpt-oss-20b，每百万
    // token）；缓存自动生效、命中部分五折（console.groq.com/docs/prompt-caching），
    // 20b 的缓存价按页面显示值 0.037 填。窗口与最大输出同 OpenCode 预设
    settingsConfig: {
      baseUrl: "https://api.groq.com/openai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "openai/gpt-oss-120b",
          name: "GPT-OSS 120B",
          reasoning: true,
          input: ["text"],
          contextWindow: 131072,
          maxTokens: 65536,
          cost: { input: 0.15, output: 0.6, cacheRead: 0.075, cacheWrite: 0 },
        },
        {
          id: "openai/gpt-oss-20b",
          name: "GPT-OSS 20B",
          reasoning: true,
          input: ["text"],
          contextWindow: 131072,
          maxTokens: 65536,
          cost: { input: 0.075, output: 0.3, cacheRead: 0.037, cacheWrite: 0 },
        },
      ],
    },
    category: "aggregator",
    icon: "groq",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "groq/openai/gpt-oss-120b" },
      modelCatalog: {
        "groq/openai/gpt-oss-120b": { alias: "GPT-OSS 120B" },
      },
    },
  },
  {
    name: "SambaNova",
    websiteUrl: "https://sambanova.ai",
    apiKeyUrl: "https://cloud.sambanova.ai/apis",
    // OpenAI 兼容 base 为 https://api.sambanova.ai/v1（docs.sambanova.ai/docs/en/
    // integrations/opencode，2026-10-14 核对），Bearer 鉴权。窗口与 cost 取公开
    // /v1/models 同日值（每百万 token，与 cloud.sambanova.ai/plans/pricing 一致）；
    // 只有 MiniMax-M3 有自动前缀缓存（docs.sambanova.ai/docs/en/features/
    // prompt-caching）。平台要求输入加输出不超窗，maxTokens 按 MiniMax 官方值
    // 131072 写，不照平台声明的 1048576
    settingsConfig: {
      baseUrl: "https://api.sambanova.ai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "MiniMax-M3",
          name: "MiniMax M3",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 0.6, output: 2.4, cacheRead: 0.06, cacheWrite: 0 },
        },
        {
          id: "gpt-oss-120b",
          name: "gpt-oss-120b",
          reasoning: true,
          input: ["text"],
          contextWindow: 131072,
          maxTokens: 131072,
          cost: { input: 0.22, output: 0.59, cacheRead: 0, cacheWrite: 0 },
        },
      ],
    },
    category: "cloud_provider",
    icon: "sambanova",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "sambanova/MiniMax-M3" },
      modelCatalog: {
        "sambanova/MiniMax-M3": { alias: "MiniMax M3" },
      },
    },
  },
  {
    name: "DigitalOcean",
    websiteUrl: "https://www.digitalocean.com/products/ai-platform",
    apiKeyUrl: "https://cloud.digitalocean.com/model-studio/manage-keys",
    // 官方 Coding Agents 文档的 OpenClaw 段（docs.digitalocean.com/products/
    // inference/how-to/use-with-coding-agents，2026-10-14 核对）原文即
    // digitalocean 供应商 + https://inference.do-ai.run/v1 + openai-completions，
    // Bearer 鉴权。窗口与最大输出取平台模型表、cost 取平台价目表（每百万
    // token，docs.digitalocean.com/products/inference/details/pricing 同日）；
    // Kimi K3 平台未公布最大输出，按 models.dev 写 131072；DeepSeek V4 Pro 0813
    // 平台表写的最大输出等于窗口，可疑，按 V4 Pro 官方值 384000 写。Sonnet 5.5
    // 须 Tier 3+ 账号，缓存写入取 5 分钟价。默认 GLM-5.3，所有等级可用
    settingsConfig: {
      baseUrl: "https://inference.do-ai.run/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "glm-5.3",
          name: "GLM-5.3",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 128000,
          cost: { input: 1.4, output: 4.4, cacheRead: 0.26, cacheWrite: 0 },
        },
        {
          id: "kimi-k3",
          name: "Kimi K3",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 0 },
        },
        {
          id: "deepseek-v4-pro-0813",
          name: "DeepSeek V4 Pro 0813",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 384000,
          cost: { input: 1.32, output: 3.96, cacheRead: 0.044, cacheWrite: 0 },
        },
        {
          id: "anthropic-claude-sonnet-5.5",
          name: "Claude Sonnet 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
      ],
    },
    category: "cloud_provider",
    icon: "digitalocean",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "digitalocean/glm-5.3" },
      modelCatalog: {
        "digitalocean/glm-5.3": { alias: "GLM-5.3" },
      },
    },
  },
  {
    name: "Hugging Face",
    websiteUrl: "https://huggingface.co/inference/models",
    apiKeyUrl:
      "https://huggingface.co/settings/tokens/new?ownUserPermissions=inference.serverless.write&tokenType=fineGrained",
    // Inference Providers 的 OpenAI Chat 端点 base 为 https://router.huggingface.co/v1
    //（docs.openclaw.ai/providers/huggingface 与 huggingface.co/docs/
    // inference-providers/index，2026-10-14 核对），Bearer 鉴权；OpenClaw 官方
    // 示例的模型引用带 :cheapest 后缀，冒号可用。上游钉 :baseten（同 Claude Code
    // 预设）；窗口、输入模态与 cost（每百万 token）取公开 /v1/models 对 Baseten
    // 的同日声明，缓存价未公布故不写 cacheRead；最大输出平台未声明，沿用仓库口径
    settingsConfig: {
      baseUrl: "https://router.huggingface.co/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "zai-org/GLM-5.3:baseten",
          name: "GLM-5.3 (Baseten)",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 1.4, output: 4.4 },
        },
        {
          id: "zai-org/GLM-5.3-Flash:baseten",
          name: "GLM-5.3 Flash (Baseten)",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 0.15, output: 0.5 },
        },
        {
          id: "moonshotai/Kimi-K3:baseten",
          name: "Kimi K3 (Baseten)",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 3, output: 15 },
        },
        {
          id: "deepseek-ai/DeepSeek-V4-Pro-0813:baseten",
          name: "DeepSeek V4 Pro 0813 (Baseten)",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 384000,
          cost: { input: 1.32, output: 3.96 },
        },
      ],
    },
    category: "aggregator",
    icon: "huggingface",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "hf_...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "huggingface/zai-org/GLM-5.3:baseten" },
      modelCatalog: {
        "huggingface/zai-org/GLM-5.3:baseten": { alias: "GLM-5.3" },
      },
    },
  },
  {
    name: "Microsoft Foundry",
    websiteUrl: "https://azure.microsoft.com/en-us/products/ai-foundry",
    apiKeyUrl: "https://ai.azure.com/",
    // OpenClaw 官方 Microsoft Foundry 文档（docs.openclaw.ai/plugins/reference/
    // microsoft-foundry.md，2026-10-14 核对）：Foundry 上的 Claude 要配成自定义
    // anthropic-messages 供应商；内置插件占用 microsoft-foundry 这个 ID，供应商
    // 名请另取（下面引用用 foundry-claude）。base 同 Claude Code 预设，为
    // https://<资源名>.services.ai.azure.com/anthropic、不带 /v1，API Key 发
    // x-api-key（learn.microsoft.com/en-us/azure/foundry/foundry-models/how-to/
    // use-foundry-models-claude 同日），资源名用字面占位符。模型 ID 填默认部署名；
    // 部署名与模型 ID 不一致时须在模型上设 params.canonicalModelId，否则思考档位
    // 处理会错。窗口与最大输出取微软模型表（.../concepts/claude-models 同日），
    // cost（每百万 token）取 Anthropic 官方价（Foundry 按同价折 CCU），缓存写入
    // 取 5 分钟价；Haiku 5.5 取提示词 10 万 token 以内的标准档
    settingsConfig: {
      baseUrl: "https://YOUR_RESOURCE_NAME.services.ai.azure.com/anthropic",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 4, output: 20, cacheRead: 0.2, cacheWrite: 5 },
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 2, output: 10, cacheRead: 0.1, cacheWrite: 2.5 },
        },
        {
          id: "claude-haiku-5-5",
          name: "Claude Haiku 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 0.1, output: 0.5, cacheRead: 0.01, cacheWrite: 0.125 },
        },
      ],
    },
    category: "cloud_provider",
    icon: "azure",
    iconColor: "#0078D4",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "foundry-claude/claude-opus-5-5",
        fallbacks: ["foundry-claude/claude-sonnet-5-5"],
      },
      modelCatalog: {
        "foundry-claude/claude-opus-5-5": { alias: "Opus" },
        "foundry-claude/claude-sonnet-5-5": { alias: "Sonnet" },
      },
    },
  },
  {
    name: "Fireworks AI",
    websiteUrl: "https://fireworks.ai",
    apiKeyUrl: "https://app.fireworks.ai/settings/users/api-keys",
    // OpenClaw 官方 Fireworks 文档（docs.openclaw.ai/providers/fireworks，
    // 2026-10-14 核对）：base 为 https://api.fireworks.ai/inference/v1、
    // openai-completions，Bearer 鉴权，模型引用写 fireworks/accounts/fireworks/
    // models/<id>。官方 Fireworks 插件（须另装）占用 fireworks / fireworks-ai
    // 两个 ID，装了插件的话供应商名请另取。引用前缀不能省：rebase 在第一个 /
    // 处切开，省了会把 accounts 当成供应商键。窗口、输入模态取各模型页，cost
    //（每百万 token，标准档）取 docs.fireworks.ai/serverless/pricing 同日；
    // 定价页没列缓存写入价，写 0。最大输出平台写"可到整个上下文"，保守按 131072
    // 写，DeepSeek V4.1 Flash 取 384000；MiniMax M3 只开 512K、不收图片
    settingsConfig: {
      baseUrl: "https://api.fireworks.ai/inference/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "accounts/fireworks/models/glm-5p3",
          name: "GLM 5.3",
          reasoning: true,
          input: ["text"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 1.4, output: 4.4, cacheRead: 0.26, cacheWrite: 0 },
        },
        {
          id: "accounts/fireworks/models/kimi-k3",
          name: "Kimi K3",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 0 },
        },
        {
          id: "accounts/fireworks/models/deepseek-v4p1-flash",
          name: "DeepSeek V4.1 Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 384000,
          cost: { input: 0.3, output: 1.2, cacheRead: 0.006, cacheWrite: 0 },
        },
        {
          id: "accounts/fireworks/models/glm-5p3-flash",
          name: "GLM 5.3 Flash",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 0.15, output: 0.5, cacheRead: 0.03, cacheWrite: 0 },
        },
        {
          id: "accounts/fireworks/models/minimax-m3",
          name: "MiniMax M3",
          reasoning: true,
          input: ["text"],
          contextWindow: 512000,
          maxTokens: 131072,
          cost: { input: 0.3, output: 1.2, cacheRead: 0.06, cacheWrite: 0 },
        },
      ],
    },
    category: "cloud_provider",
    icon: "fireworks",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "fw_...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "fireworks/accounts/fireworks/models/glm-5p3" },
      modelCatalog: {
        "fireworks/accounts/fireworks/models/glm-5p3": { alias: "GLM 5.3" },
      },
    },
  },
  {
    name: "Mistral AI",
    websiteUrl: "https://mistral.ai",
    apiKeyUrl: "https://console.mistral.ai/api-keys",
    // OpenAI Chat 端点 base 为 https://api.mistral.ai/v1（docs.mistral.ai/
    // openapi.yaml，2026-10-14 核对），Bearer 鉴权。OpenClaw 的 Mistral 支持是
    // 须另装的官方插件（docs.openclaw.ai/providers/mistral 同日），装了插件的话
    // 供应商名请另取。模型用官方模型表（docs.mistral.ai/models.md 同日）的固定
    // 版本 ID，窗口官方写 256k；cost（每百万 token）取 docs.mistral.ai/inference/
    // pricing.md 同日，缓存读按官方「缓存 token 按输入价 10% 计」
    //（docs.mistral.ai/studio/conversations/advanced/prompt-caching.md）推算，
    // 不收缓存写入费，写 0。官方没有单独的最大输出，规则是 prompt + max_tokens
    // ≤ 窗口，保守按 32768 写
    settingsConfig: {
      baseUrl: "https://api.mistral.ai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "mistral-medium-3-5",
          name: "Mistral Medium 3.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 256000,
          maxTokens: 32768,
          cost: { input: 1.5, output: 7.5, cacheRead: 0.15, cacheWrite: 0 },
        },
        {
          id: "mistral-small-2603",
          name: "Mistral Small 4",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 256000,
          maxTokens: 32768,
          cost: { input: 0.15, output: 0.6, cacheRead: 0.015, cacheWrite: 0 },
        },
      ],
    },
    category: "cn_official",
    icon: "mistral",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "mistral/mistral-medium-3-5" },
      modelCatalog: {
        "mistral/mistral-medium-3-5": { alias: "Mistral Medium 3.5" },
      },
    },
  },
  {
    name: "Cloudflare AI Gateway",
    websiteUrl: "https://developers.cloudflare.com/ai-gateway/",
    apiKeyUrl: "https://dash.cloudflare.com/profile/api-tokens",
    // 走 REST 统一 API（developers.cloudflare.com/ai-gateway/usage/rest-api，
    // 2026-05-21 上线，2026-10-14 核对）：base 为 https://api.cloudflare.com/
    // client/v4/accounts/<账户 ID>/ai/v1，标准 Bearer 鉴权，默认走 default 网关；
    // 该页要求 token 带 Workers AI Read（只有 AI Gateway 权限会 401）。表单不替换
    // 模板变量，账户 ID 用字面占位符，忘了改会 401。旧的 /compat 端点已废弃不用。
    // OpenClaw 官方插件 @openclaw/cloudflare-ai-gateway-provider 用的 provider ID
    // 是 cloudflare-ai-gateway（docs.openclaw.ai/providers/cloudflare-ai-gateway
    // 同日），供应商名请另取（如 cloudflare）免得撞车。模型 ID 为 author/model、
    // 用点号；Claude 系列在官方目录（developers.cloudflare.com/ai/models 同日）只
    // 写 Anthropic Messages 格式，Chat 端点不放。窗口、最大输出与 cost（每百万
    // token）取该目录与 models.dev 的 cloudflare-ai-gateway 条目同日，GPT 取 272K
    // 内的单价；缓存写入未列，不写。带工具调用加流式的实际行为未经真 Key 实测
    settingsConfig: {
      baseUrl:
        "https://api.cloudflare.com/client/v4/accounts/YOUR_ACCOUNT_ID/ai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "openai/gpt-6.1-sol",
          name: "GPT-6.1 Sol",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 2, output: 10, cacheRead: 0.1 },
        },
        {
          id: "openai/gpt-6-luna",
          name: "GPT-6 Luna",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 0.1, output: 0.5, cacheRead: 0.01 },
        },
        {
          id: "moonshotai/kimi-k3",
          name: "Kimi K3",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1048576,
          maxTokens: 131072,
          cost: { input: 3, output: 15, cacheRead: 0.3 },
        },
      ],
    },
    category: "cloud_provider",
    icon: "cloudflare",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "cloudflare/openai/gpt-6.1-sol" },
      modelCatalog: {
        "cloudflare/openai/gpt-6.1-sol": { alias: "GPT-6.1 Sol" },
      },
    },
  },
  {
    name: "PIPELLM",
    websiteUrl: "https://code.pipellm.ai",
    apiKeyUrl: "https://code.pipellm.ai/login?ref=uvw650za",
    settingsConfig: {
      baseUrl: "https://cc-api.pipellm.ai",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5",
          name: "claude-opus-5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
        {
          id: "claude-sonnet-5",
          name: "claude-sonnet-5",
          contextWindow: 1000000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "claude-haiku-4-5-20251001",
          contextWindow: 200000,
          cost: { input: 0.8, output: 4 },
        },
      ],
    },
    category: "aggregator",
    icon: "pipellm",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "pipe-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "pipellm/claude-opus-5",
        fallbacks: ["pipellm/claude-sonnet-5"],
      },
      modelCatalog: {
        "pipellm/claude-opus-5": { alias: "Opus" },
        "pipellm/claude-sonnet-5": { alias: "Sonnet" },
        "pipellm/claude-haiku-4-5-20251001": { alias: "Haiku" },
      },
    },
  },
  {
    name: "E-FlowCode",
    websiteUrl: "https://e-flowcode.cc",
    apiKeyUrl: "https://e-flowcode.cc",
    settingsConfig: {
      api: "openai-responses",
      apiKey: "",
      baseUrl: "https://e-flowcode.cc/v1",
      headers: {
        "User-Agent":
          "codex_cli_rs/0.77.0 (Windows 10.0.26100; x86_64) WindowsTerminal",
      },
      models: [
        {
          contextWindow: 200000,
          cost: {
            cacheRead: 0,
            cacheWrite: 0,
            input: 0,
            output: 0,
          },
          id: "gpt-5.3-codex",
          maxTokens: 32000,
          name: "gpt-5.3-codex",
        },
        {
          id: "gpt-5.6-sol",
          name: "gpt-5.6-sol",
        },
        {
          id: "gpt-5.2-codex",
          name: "gpt-5.2-codex",
        },
        {
          id: "gpt-5.2",
          name: "gpt-5.2",
        },
        {
          id: "gpt-6-sol",
          name: "GPT-6 Sol",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
        {
          id: "gpt-6-astra",
          name: "GPT-6 Astra",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 1, cacheWrite: 12.5 },
        },
      ],
    },
    category: "third_party",
    icon: "eflowcode",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "eflowcode/gpt-5.3-codex",
        fallbacks: ["eflowcode/gpt-5.6-sol", "eflowcode/gpt-5.2-codex"],
      },
      modelCatalog: {
        "eflowcode/gpt-5.3-codex": { alias: "gpt-5.3-codex" },
        "eflowcode/gpt-5.6-sol": { alias: "gpt-5.6-sol" },
        "eflowcode/gpt-5.2-codex": { alias: "gpt-5.2-codex" },
        "eflowcode/gpt-5.2": { alias: "gpt-5.2" },
      },
    },
  },
  {
    name: "AWS Bedrock",
    websiteUrl: "https://aws.amazon.com/bedrock/",
    settingsConfig: {
      // 请将 us-west-2 替换为你的 AWS Region
      baseUrl: "https://bedrock-runtime.us-west-2.amazonaws.com",
      apiKey: "",
      api: "bedrock-converse-stream",
      models: [
        {
          id: "anthropic.claude-opus-5",
          name: "Claude Opus 5",
          contextWindow: 1000000,
          cost: { input: 5, output: 25, cacheRead: 0.5, cacheWrite: 6.25 },
        },
        {
          id: "anthropic.claude-sonnet-5",
          name: "Claude Sonnet 5",
          contextWindow: 1000000,
          cost: { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 3.75 },
        },
        {
          id: "anthropic.claude-haiku-4-5-20251022-v1:0",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
          cost: { input: 0.8, output: 4, cacheRead: 0.08, cacheWrite: 1 },
        },
      ],
    },
    category: "cloud_provider",
    icon: "aws",
    iconColor: "#FF9900",
  },
  {
    name: "JieKou AI",
    websiteUrl: "https://jiekou.ai/#model-library",
    apiKeyUrl: "https://jiekou.ai/settings/key-management",
    settingsConfig: {
      baseUrl: "https://api.jiekou.ai/openai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "claude-fable-5",
          name: "Claude Fable 5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 1, cacheWrite: 12.5 },
        },
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          // 供应商未公布缓存写入价，按 Anthropic 标准参考价估算。
          cost: { input: 4, output: 20, cacheRead: 0.2, cacheWrite: 5 },
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 0.25, cacheWrite: 12.5 },
        },
      ],
    },
    category: "aggregator",
    icon: "jiekou",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "jiekou/claude-fable-5" },
      modelCatalog: {
        "jiekou/claude-fable-5": { alias: "Claude Fable 5" },
      },
    },
  },
  {
    name: "AICodeWith",
    websiteUrl: "https://aicodewith.ai",
    apiKeyUrl: "https://aicodewith.ai/login?tab=register",
    settingsConfig: {
      baseUrl: "https://api.aicodewith.ai/v1",
      apiKey: "",
      api: "openai-responses",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
        },
        {
          id: "gpt-6-sol",
          name: "GPT-6 Sol",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 2, output: 10, cacheRead: 0.2, cacheWrite: 2.5 },
        },
        {
          id: "gpt-6-astra",
          name: "GPT-6 Astra",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1050000,
          maxTokens: 128000,
          cost: { input: 10, output: 50, cacheRead: 1, cacheWrite: 12.5 },
        },
      ],
    },
    category: "aggregator",
    icon: "aicodewith",
    iconColor: "#3A3B40",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "aicodewith/gpt-5.6-sol" },
      modelCatalog: {
        "aicodewith/gpt-5.6-sol": { alias: "GPT-5.6 Sol" },
      },
    },
  },
  {
    name: "模力方舟",
    websiteUrl: "https://moark.com",
    apiKeyUrl: "https://moark.com/dashboard/tokens",
    settingsConfig: {
      baseUrl: "https://api.moark.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "deepseek-v4-flash-0731",
          name: "DeepSeek V4 Flash",
          reasoning: true,
          contextWindow: 1000000,
          maxTokens: 384000,
        },
        {
          id: "DeepSeek-V4-Pro",
          name: "DeepSeek V4 Pro",
          reasoning: true,
          contextWindow: 1000000,
          maxTokens: 384000,
        },
        {
          id: "GLM-5.3",
          name: "GLM-5.3",
          reasoning: true,
          contextWindow: 1048576,
          maxTokens: 131072,
        },
        {
          id: "Kimi-K2.7-Code",
          name: "Kimi K2.7 Code",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 262144,
          maxTokens: 262144,
        },
        {
          id: "qwen3-coder-plus",
          name: "Qwen3 Coder Plus",
          contextWindow: 1000000,
          maxTokens: 65536,
        },
      ],
    },
    category: "aggregator",
    icon: "moark",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "moark/deepseek-v4-flash-0731" },
      modelCatalog: {
        "moark/deepseek-v4-flash-0731": { alias: "DeepSeek V4 Flash" },
      },
    },
  },
  {
    name: "SoruxGPT",
    websiteUrl: "https://ai.soruxgpt.com",
    apiKeyUrl: "https://ai.soruxgpt.com/console/token",
    settingsConfig: {
      baseUrl: "https://ai.soruxgpt.com",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
      ],
    },
    category: "aggregator",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "soruxgpt/claude-opus-5-5",
        fallbacks: ["soruxgpt/claude-sonnet-5-5"],
      },
      modelCatalog: {
        "soruxgpt/claude-opus-5-5": { alias: "Opus" },
        "soruxgpt/claude-sonnet-5-5": { alias: "Sonnet" },
        "soruxgpt/claude-haiku-4-5-20251001": { alias: "Haiku" },
      },
    },
    icon: "soruxgpt",
  },
  {
    name: "IKunCode",
    websiteUrl: "https://api.ikuncode.ai",
    apiKeyUrl: "https://api.ikuncode.ai/console/token",
    settingsConfig: {
      baseUrl: "https://api.ikuncode.ai",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
      ],
    },
    category: "aggregator",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "ikuncode/claude-opus-5-5",
        fallbacks: ["ikuncode/claude-sonnet-5-5"],
      },
      modelCatalog: {
        "ikuncode/claude-opus-5-5": { alias: "Opus" },
        "ikuncode/claude-sonnet-5-5": { alias: "Sonnet" },
        "ikuncode/claude-haiku-4-5-20251001": { alias: "Haiku" },
      },
    },
    icon: "ikuncode",
  },
  {
    name: "RunToken",
    websiteUrl: "https://www.runtoken.ai",
    apiKeyUrl: "https://www.runtoken.ai/console/token",
    settingsConfig: {
      baseUrl: "https://www.runtoken.ai",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
      ],
    },
    category: "aggregator",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "runtoken/claude-opus-5-5",
        fallbacks: ["runtoken/claude-sonnet-5-5"],
      },
      modelCatalog: {
        "runtoken/claude-opus-5-5": { alias: "Opus" },
        "runtoken/claude-sonnet-5-5": { alias: "Sonnet" },
        "runtoken/claude-haiku-4-5-20251001": { alias: "Haiku" },
      },
    },
    icon: "runtoken",
  },
  {
    name: "BotCF",
    websiteUrl: "https://botcf.com",
    apiKeyUrl: "https://botcf.com/console/token",
    settingsConfig: {
      baseUrl: "https://botcf.com",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
      ],
    },
    category: "aggregator",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "botcf/claude-opus-5-5",
        fallbacks: ["botcf/claude-sonnet-5-5"],
      },
      modelCatalog: {
        "botcf/claude-opus-5-5": { alias: "Opus" },
        "botcf/claude-sonnet-5-5": { alias: "Sonnet" },
        "botcf/claude-haiku-4-5-20251001": { alias: "Haiku" },
      },
    },
    icon: "botcf",
  },
  {
    name: "YYLX",
    nameKey: "providerForm.presets.yylx",
    websiteUrl: "https://yylx.io",
    apiKeyUrl: "https://app.yylx.io/keys",
    settingsConfig: {
      baseUrl: "https://app.yylx.io",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 64000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
      ],
    },
    category: "aggregator",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "yylx/claude-opus-5-5",
        fallbacks: ["yylx/claude-sonnet-5"],
      },
      modelCatalog: {
        "yylx/claude-opus-5-5": { alias: "Opus" },
        "yylx/claude-sonnet-5": { alias: "Sonnet" },
        "yylx/claude-haiku-4-5-20251001": { alias: "Haiku" },
      },
    },
    icon: "yylx",
  },
  {
    name: "TokenFlux",
    websiteUrl: "https://tokenflux.dev",
    apiKeyUrl: "https://tokenflux.dev/keys",
    settingsConfig: {
      baseUrl: "https://tokenflux.dev",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-haiku-4-5",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
      ],
    },
    category: "aggregator",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "tokenflux/claude-opus-5-5",
        fallbacks: ["tokenflux/claude-sonnet-5-5"],
      },
      modelCatalog: {
        "tokenflux/claude-opus-5-5": { alias: "Opus" },
        "tokenflux/claude-sonnet-5-5": { alias: "Sonnet" },
        "tokenflux/claude-haiku-4-5": { alias: "Haiku" },
      },
    },
    icon: "tokenflux",
  },
  {
    name: "NekoCode",
    websiteUrl: "https://nekoapi.ai",
    apiKeyUrl: "https://nekoapi.ai/token",
    settingsConfig: {
      baseUrl: "https://nekoapi.ai",
      apiKey: "",
      api: "anthropic-messages",
      headers: {
        "User-Agent": "claude-cli/2.1.161 (external, cli)",
      },
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
      ],
    },
    category: "aggregator",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "nekocode/claude-opus-5-5",
        fallbacks: ["nekocode/claude-sonnet-5-5"],
      },
      modelCatalog: {
        "nekocode/claude-opus-5-5": { alias: "Opus" },
        "nekocode/claude-sonnet-5-5": { alias: "Sonnet" },
        "nekocode/claude-haiku-4-5-20251001": { alias: "Haiku" },
      },
    },
    icon: "nekocode",
  },
  {
    name: "AIComing",
    websiteUrl: "https://aicoming.top",
    apiKeyUrl: "https://aicoming.top/console#keys",
    settingsConfig: {
      baseUrl: "https://api.aicoming.top",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          contextWindow: 200000,
        },
      ],
    },
    category: "aggregator",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "aicoming/claude-opus-5-5",
        fallbacks: ["aicoming/claude-sonnet-5-5"],
      },
      modelCatalog: {
        "aicoming/claude-opus-5-5": { alias: "Opus" },
        "aicoming/claude-sonnet-5-5": { alias: "Sonnet" },
        "aicoming/claude-haiku-4-5-20251001": { alias: "Haiku" },
      },
    },
    icon: "aicoming",
  },
];
