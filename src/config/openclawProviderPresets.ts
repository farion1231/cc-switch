/**
 * OpenClaw provider presets configuration
 * OpenClaw uses models.providers structure with custom provider configs
 */
import type {
  ProviderCategory,
  OpenClawProviderConfig,
  OpenClawDefaultModel,
} from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

/** Suggested default model configuration for a preset */
export interface OpenClawSuggestedDefaults {
  /** Default model config to apply (agents.defaults.model) */
  model?: OpenClawDefaultModel;
  /** Model catalog entries to add (agents.defaults.models) */
  modelCatalog?: Record<string, { alias?: string }>;
}

export interface OpenClawProviderPreset {
  name: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  /** OpenClaw settings_config structure */
  settingsConfig: OpenClawProviderConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
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
  // ========== Chinese Officials ==========
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
          id: "deepseek-chat",
          name: "DeepSeek V3.2",
          contextWindow: 64000,
          cost: { input: 0.0005, output: 0.002 },
        },
        {
          id: "deepseek-reasoner",
          name: "DeepSeek R1",
          contextWindow: 64000,
          cost: { input: 0.0005, output: 0.002 },
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
        primary: "deepseek/deepseek-chat",
        fallbacks: ["deepseek/deepseek-reasoner"],
      },
      modelCatalog: {
        "deepseek/deepseek-chat": { alias: "DeepSeek" },
        "deepseek/deepseek-reasoner": { alias: "R1" },
      },
    },
  },
  {
    name: "Zhipu GLM",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    settingsConfig: {
      baseUrl: "https://open.bigmodel.cn/api/paas/v4",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "glm-5",
          name: "GLM-5",
          contextWindow: 128000,
          cost: { input: 0.001, output: 0.001 },
        },
      ],
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://open.bigmodel.cn/api/paas/v4",
        defaultValue: "https://open.bigmodel.cn/api/paas/v4",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "zhipu/glm-5" },
      modelCatalog: { "zhipu/glm-5": { alias: "GLM" } },
    },
  },
  {
    name: "Zhipu GLM en",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe?ic=8JVLJQFSKB",
    settingsConfig: {
      baseUrl: "https://api.z.ai/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "glm-5",
          name: "GLM-5",
          contextWindow: 128000,
          cost: { input: 0.001, output: 0.001 },
        },
      ],
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.z.ai/v1",
        defaultValue: "https://api.z.ai/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "zhipu-en/glm-5" },
      modelCatalog: { "zhipu-en/glm-5": { alias: "GLM" } },
    },
  },
  {
    name: "Qwen Coder",
    websiteUrl: "https://bailian.console.aliyun.com",
    apiKeyUrl: "https://bailian.console.aliyun.com/#/api-key",
    settingsConfig: {
      baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "qwen3.5-plus",
          name: "Qwen3.5 Plus",
          contextWindow: 32000,
          cost: { input: 0.002, output: 0.006 },
        },
      ],
    },
    category: "cn_official",
    icon: "qwen",
    iconColor: "#FF6A00",
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
      model: { primary: "qwen/qwen3.5-plus" },
      modelCatalog: { "qwen/qwen3.5-plus": { alias: "Qwen" } },
    },
  },
  {
    name: "Kimi k2.5",
    websiteUrl: "https://platform.moonshot.cn/console",
    apiKeyUrl: "https://platform.moonshot.cn/console/api-keys",
    settingsConfig: {
      baseUrl: "https://api.moonshot.cn/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "kimi-k2.5",
          name: "Kimi K2.5",
          contextWindow: 131072,
          cost: { input: 0.002, output: 0.006 },
        },
      ],
    },
    category: "cn_official",
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
      model: { primary: "kimi/kimi-k2.5" },
      modelCatalog: { "kimi/kimi-k2.5": { alias: "Kimi" } },
    },
  },
  {
    name: "Kimi For Coding",
    websiteUrl: "https://www.kimi.com/coding/docs/",
    apiKeyUrl: "https://platform.moonshot.cn/console/api-keys",
    settingsConfig: {
      baseUrl: "https://api.kimi.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "kimi-for-coding",
          name: "Kimi For Coding",
          contextWindow: 131072,
          cost: { input: 0.002, output: 0.006 },
        },
      ],
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.kimi.com/v1",
        defaultValue: "https://api.kimi.com/v1",
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
    name: "MiniMax",
    websiteUrl: "https://platform.minimaxi.com",
    apiKeyUrl: "https://platform.minimaxi.com/subscribe/coding-plan",
    settingsConfig: {
      baseUrl: "https://api.minimaxi.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "MiniMax-M2.5",
          name: "MiniMax M2.5",
          contextWindow: 200000,
          cost: { input: 0.001, output: 0.004 },
        },
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "minimax_cn",
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
      model: { primary: "minimax/MiniMax-M2.5" },
      modelCatalog: { "minimax/MiniMax-M2.5": { alias: "MiniMax" } },
    },
  },
  {
    name: "MiniMax en",
    websiteUrl: "https://platform.minimax.io",
    apiKeyUrl: "https://platform.minimax.io/subscribe/coding-plan",
    settingsConfig: {
      baseUrl: "https://api.minimax.io/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "MiniMax-M2.5",
          name: "MiniMax M2.5",
          contextWindow: 200000,
          cost: { input: 0.001, output: 0.004 },
        },
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "minimax_en",
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
      model: { primary: "minimax-en/MiniMax-M2.5" },
      modelCatalog: { "minimax-en/MiniMax-M2.5": { alias: "MiniMax" } },
    },
  },
  {
    name: "KAT-Coder",
    websiteUrl: "https://console.streamlake.ai",
    apiKeyUrl: "https://console.streamlake.ai/console/api-key",
    settingsConfig: {
      baseUrl:
        "https://vanchin.streamlake.ai/api/gateway/v1/endpoints/${ENDPOINT_ID}/openai",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "KAT-Coder-Pro",
          name: "KAT-Coder Pro",
          contextWindow: 128000,
          cost: { input: 0.002, output: 0.006 },
        },
      ],
    },
    category: "cn_official",
    icon: "catcoder",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder:
          "https://vanchin.streamlake.ai/api/gateway/v1/endpoints/${ENDPOINT_ID}/openai",
        defaultValue:
          "https://vanchin.streamlake.ai/api/gateway/v1/endpoints/${ENDPOINT_ID}/openai",
        editorValue: "",
      },
      ENDPOINT_ID: {
        label: "Endpoint ID",
        placeholder: "",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "katcoder/KAT-Coder-Pro" },
      modelCatalog: { "katcoder/KAT-Coder-Pro": { alias: "KAT-Coder" } },
    },
  },
  {
    name: "Longcat",
    websiteUrl: "https://longcat.chat/platform",
    apiKeyUrl: "https://longcat.chat/platform/api_keys",
    settingsConfig: {
      baseUrl: "https://api.longcat.chat/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "LongCat-Flash-Chat",
          name: "LongCat Flash Chat",
          contextWindow: 128000,
          cost: { input: 0.001, output: 0.004 },
        },
      ],
    },
    category: "cn_official",
    icon: "longcat",
    iconColor: "#29E154",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.longcat.chat/v1",
        defaultValue: "https://api.longcat.chat/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "longcat/LongCat-Flash-Chat" },
      modelCatalog: { "longcat/LongCat-Flash-Chat": { alias: "LongCat" } },
    },
  },
  {
    name: "DouBaoSeed",
    websiteUrl: "https://www.volcengine.com/product/doubao",
    apiKeyUrl: "https://www.volcengine.com/product/doubao",
    settingsConfig: {
      baseUrl: "https://ark.cn-beijing.volces.com/api/v3",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "doubao-seed-2-0-code-preview-latest",
          name: "DouBao Seed Code Preview",
          contextWindow: 128000,
          cost: { input: 0.002, output: 0.006 },
        },
      ],
    },
    category: "cn_official",
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
      model: { primary: "doubaoseed/doubao-seed-2-0-code-preview-latest" },
      modelCatalog: {
        "doubaoseed/doubao-seed-2-0-code-preview-latest": { alias: "DouBao" },
      },
    },
  },
  {
    name: "BaiLing",
    websiteUrl: "https://alipaytbox.yuque.com/sxs0ba/ling/get_started",
    settingsConfig: {
      baseUrl: "https://api.tbox.cn/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "Ling-2.5-1T",
          name: "Ling 2.5 1T",
          contextWindow: 128000,
          cost: { input: 0.001, output: 0.004 },
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
      model: { primary: "bailing/Ling-2.5-1T" },
      modelCatalog: { "bailing/Ling-2.5-1T": { alias: "BaiLing" } },
    },
  },
  {
    name: "Xiaomi MiMo",
    websiteUrl: "https://platform.xiaomimimo.com",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/api-keys",
    settingsConfig: {
      baseUrl: "https://api.xiaomimimo.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "mimo-v2-flash",
          name: "MiMo V2 Flash",
          contextWindow: 128000,
          cost: { input: 0.001, output: 0.004 },
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
      model: { primary: "xiaomimimo/mimo-v2-flash" },
      modelCatalog: { "xiaomimimo/mimo-v2-flash": { alias: "MiMo" } },
    },
  },

  // ========== Aggregators ==========
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
          id: "claude-sonnet-4-6",
          name: "Claude Sonnet 4.6",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-6",
          name: "Claude Opus 4.6",
          contextWindow: 200000,
          cost: { input: 5, output: 25 },
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
        primary: "aihubmix/claude-sonnet-4-6",
        fallbacks: ["aihubmix/claude-opus-4-6"],
      },
      modelCatalog: {
        "aihubmix/claude-sonnet-4-6": { alias: "Sonnet" },
        "aihubmix/claude-opus-4-6": { alias: "Opus" },
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
          id: "claude-sonnet-4-6",
          name: "Claude Sonnet 4.6",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-6",
          name: "Claude Opus 4.6",
          contextWindow: 200000,
          cost: { input: 5, output: 25 },
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
        primary: "dmxapi/claude-sonnet-4-6",
        fallbacks: ["dmxapi/claude-opus-4-6"],
      },
      modelCatalog: {
        "dmxapi/claude-sonnet-4-6": { alias: "Sonnet" },
        "dmxapi/claude-opus-4-6": { alias: "Opus" },
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
          id: "anthropic/claude-sonnet-4.6",
          name: "Claude Sonnet 4.6",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "anthropic/claude-opus-4.6",
          name: "Claude Opus 4.6",
          contextWindow: 200000,
          cost: { input: 5, output: 25 },
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
        primary: "openrouter/anthropic/claude-sonnet-4.6",
        fallbacks: ["openrouter/anthropic/claude-opus-4.6"],
      },
      modelCatalog: {
        "openrouter/anthropic/claude-sonnet-4.6": { alias: "Sonnet" },
        "openrouter/anthropic/claude-opus-4.6": { alias: "Opus" },
      },
    },
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
          id: "ZhipuAI/GLM-5",
          name: "GLM-5",
          contextWindow: 128000,
          cost: { input: 0.001, output: 0.001 },
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
      model: { primary: "modelscope/ZhipuAI/GLM-5" },
      modelCatalog: { "modelscope/ZhipuAI/GLM-5": { alias: "GLM" } },
    },
  },
  {
    name: "SiliconFlow",
    websiteUrl: "https://siliconflow.cn",
    apiKeyUrl: "https://cloud.siliconflow.cn/me/account/ak",
    settingsConfig: {
      baseUrl: "https://api.siliconflow.cn/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "Pro/MiniMaxAI/MiniMax-M2.5",
          name: "MiniMax M2.5",
          contextWindow: 200000,
          cost: { input: 0.001, output: 0.004 },
        },
      ],
    },
    category: "aggregator",
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
    websiteUrl: "https://siliconflow.com",
    apiKeyUrl: "https://cloud.siliconflow.com/account/ak",
    settingsConfig: {
      baseUrl: "https://api.siliconflow.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "MiniMaxAI/MiniMax-M2.5",
          name: "MiniMax M2.5",
          contextWindow: 200000,
          cost: { input: 0.001, output: 0.004 },
        },
      ],
    },
    category: "aggregator",
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
      model: { primary: "siliconflow-en/MiniMaxAI/MiniMax-M2.5" },
      modelCatalog: {
        "siliconflow-en/MiniMaxAI/MiniMax-M2.5": { alias: "MiniMax" },
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
          id: "moonshotai/kimi-k2.5",
          name: "Kimi K2.5",
          contextWindow: 131072,
          cost: { input: 0.002, output: 0.006 },
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
      model: { primary: "nvidia/moonshotai/kimi-k2.5" },
      modelCatalog: { "nvidia/moonshotai/kimi-k2.5": { alias: "Kimi" } },
    },
  },

  // ========== Third Party Partners ==========
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.com",
    apiKeyUrl: "https://www.packyapi.com/register?aff=cc-switch",
    settingsConfig: {
      baseUrl: "https://www.packyapi.com",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-sonnet-4-6",
          name: "Claude Sonnet 4.6",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-6",
          name: "Claude Opus 4.6",
          contextWindow: 200000,
          cost: { input: 5, output: 25 },
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
        primary: "packycode/claude-sonnet-4-6",
        fallbacks: ["packycode/claude-opus-4-6"],
      },
      modelCatalog: {
        "packycode/claude-sonnet-4-6": { alias: "Sonnet" },
        "packycode/claude-opus-4-6": { alias: "Opus" },
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
          id: "claude-sonnet-4-6",
          name: "Claude Sonnet 4.6",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-6",
          name: "Claude Opus 4.6",
          contextWindow: 200000,
          cost: { input: 5, output: 25 },
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
        primary: "cubence/claude-sonnet-4-6",
        fallbacks: ["cubence/claude-opus-4-6"],
      },
      modelCatalog: {
        "cubence/claude-sonnet-4-6": { alias: "Sonnet" },
        "cubence/claude-opus-4-6": { alias: "Opus" },
      },
    },
  },
  {
    name: "AIGoCode",
    websiteUrl: "https://aigocode.com",
    apiKeyUrl: "https://aigocode.com/invite/CC-SWITCH",
    settingsConfig: {
      baseUrl: "https://api.aigocode.com",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-sonnet-4-6",
          name: "Claude Sonnet 4.6",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-6",
          name: "Claude Opus 4.6",
          contextWindow: 200000,
          cost: { input: 5, output: 25 },
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
        primary: "aigocode/claude-sonnet-4-6",
        fallbacks: ["aigocode/claude-opus-4-6"],
      },
      modelCatalog: {
        "aigocode/claude-sonnet-4-6": { alias: "Sonnet" },
        "aigocode/claude-opus-4-6": { alias: "Opus" },
      },
    },
  },
  {
    name: "RightCode",
    websiteUrl: "https://www.right.codes",
    apiKeyUrl: "https://www.right.codes/register?aff=CCSWITCH",
    settingsConfig: {
      baseUrl: "https://www.right.codes/claude",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-sonnet-4-6",
          name: "Claude Sonnet 4.6",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-6",
          name: "Claude Opus 4.6",
          contextWindow: 200000,
          cost: { input: 5, output: 25 },
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
        primary: "rightcode/claude-sonnet-4-6",
        fallbacks: ["rightcode/claude-opus-4-6"],
      },
      modelCatalog: {
        "rightcode/claude-sonnet-4-6": { alias: "Sonnet" },
        "rightcode/claude-opus-4-6": { alias: "Opus" },
      },
    },
  },
  {
    name: "AICodeMirror",
    websiteUrl: "https://www.aicodemirror.com",
    apiKeyUrl: "https://www.aicodemirror.com/register?invitecode=9915W3",
    settingsConfig: {
      baseUrl: "https://api.aicodemirror.com/api/claudecode",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-sonnet-4-6",
          name: "Claude Sonnet 4.6",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-6",
          name: "Claude Opus 4.6",
          contextWindow: 200000,
          cost: { input: 5, output: 25 },
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
        primary: "aicodemirror/claude-sonnet-4-6",
        fallbacks: ["aicodemirror/claude-opus-4-6"],
      },
      modelCatalog: {
        "aicodemirror/claude-sonnet-4-6": { alias: "Sonnet" },
        "aicodemirror/claude-opus-4-6": { alias: "Opus" },
      },
    },
  },
  {
    name: "AICoding",
    websiteUrl: "https://www.aicoding.sh",
    apiKeyUrl: "https://www.aicoding.sh/i/CCSWITCH",
    settingsConfig: {
      baseUrl: "https://api.aicoding.sh",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-sonnet-4-6",
          name: "Claude Sonnet 4.6",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-6",
          name: "Claude Opus 4.6",
          contextWindow: 200000,
          cost: { input: 5, output: 25 },
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
        primary: "aicoding/claude-sonnet-4-6",
        fallbacks: ["aicoding/claude-opus-4-6"],
      },
      modelCatalog: {
        "aicoding/claude-sonnet-4-6": { alias: "Sonnet" },
        "aicoding/claude-opus-4-6": { alias: "Opus" },
      },
    },
  },
  {
    name: "CrazyRouter",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    settingsConfig: {
      baseUrl: "https://crazyrouter.com/v1",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-sonnet-4-6",
          name: "Claude Sonnet 4.6",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-6",
          name: "Claude Opus 4.6",
          contextWindow: 200000,
          cost: { input: 5, output: 25 },
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
        primary: "crazyrouter/claude-sonnet-4-6",
        fallbacks: ["crazyrouter/claude-opus-4-6"],
      },
      modelCatalog: {
        "crazyrouter/claude-sonnet-4-6": { alias: "Sonnet" },
        "crazyrouter/claude-opus-4-6": { alias: "Opus" },
      },
    },
  },
  {
    name: "SSSAiCode",
    websiteUrl: "https://www.sssaicode.com",
    apiKeyUrl: "https://www.sssaicode.com/register?ref=DCP0SM",
    settingsConfig: {
      baseUrl: "https://node-hk.sssaicode.com/api",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-sonnet-4-6",
          name: "Claude Sonnet 4.6",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-6",
          name: "Claude Opus 4.6",
          contextWindow: 200000,
          cost: { input: 5, output: 25 },
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
        primary: "sssaicode/claude-sonnet-4-6",
        fallbacks: ["sssaicode/claude-opus-4-6"],
      },
      modelCatalog: {
        "sssaicode/claude-sonnet-4-6": { alias: "Sonnet" },
        "sssaicode/claude-opus-4-6": { alias: "Opus" },
      },
    },
  },
  // ========== Cloud Providers ==========
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
          id: "anthropic.claude-opus-4-6-20250514-v1:0",
          name: "Claude Opus 4.6",
          contextWindow: 200000,
          cost: { input: 15, output: 75, cacheRead: 1.5, cacheWrite: 18.75 },
        },
        {
          id: "anthropic.claude-sonnet-4-6",
          name: "Claude Sonnet 4.6",
          contextWindow: 200000,
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

  // ========== Custom Template ==========
  {
    name: "OpenAI Compatible",
    websiteUrl: "",
    settingsConfig: {
      baseUrl: "",
      apiKey: "",
      api: "openai-completions",
      models: [],
    },
    category: "custom",
    isCustomTemplate: true,
    icon: "generic",
    iconColor: "#6B7280",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.example.com/v1",
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
