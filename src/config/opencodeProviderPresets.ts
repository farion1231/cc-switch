import type { ProviderCategory, OpenCodeProviderConfig } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

export interface OpenCodeProviderPreset {
  name: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: OpenCodeProviderConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  templateValues?: Record<string, TemplateValueConfig>;
  theme?: PresetTheme;
  icon?: string;
  iconColor?: string;
  isCustomTemplate?: boolean;
}

export const opencodeNpmPackages = [
  { value: "@ai-sdk/openai", label: "OpenAI" },
  { value: "@ai-sdk/openai-compatible", label: "OpenAI Compatible" },
  { value: "@ai-sdk/anthropic", label: "Anthropic" },
  { value: "@ai-sdk/google", label: "Google (Gemini)" },
] as const;

export interface PresetModelVariant {
  id: string;
  name?: string;
  contextLimit?: number;
  outputLimit?: number;
  modalities?: { input: string[]; output: string[] };
  options?: Record<string, unknown>;
  variants?: Record<string, Record<string, unknown>>;
}

export const OPENCODE_PRESET_MODEL_VARIANTS: Record<
  string,
  PresetModelVariant[]
> = {
  "@ai-sdk/openai-compatible": [
    {
      id: "MiniMax-M2.1",
      name: "MiniMax M2.1",
      contextLimit: 204800,
      outputLimit: 131072,
      modalities: { input: ["text"], output: ["text"] },
    },
    {
      id: "glm-4.7",
      name: "GLM 4.7",
      contextLimit: 204800,
      outputLimit: 131072,
      modalities: { input: ["text"], output: ["text"] },
    },
    {
      id: "kimi-k2.5",
      name: "Kimi K2.5",
      contextLimit: 262144,
      outputLimit: 262144,
      modalities: { input: ["text", "image", "video"], output: ["text"] },
    },
  ],
  "@ai-sdk/google": [
    {
      id: "gemini-2.5-flash-lite",
      name: "Gemini 2.5 Flash Lite",
      contextLimit: 1048576,
      outputLimit: 65536,
      modalities: {
        input: ["text", "image", "pdf", "video", "audio"],
        output: ["text"],
      },
      variants: {
        auto: {
          thinkingConfig: { includeThoughts: true, thinkingBudget: -1 },
        },
        "no-thinking": { thinkingConfig: { thinkingBudget: 0 } },
      },
    },
    {
      id: "gemini-3-flash-preview",
      name: "Gemini 3 Flash Preview",
      contextLimit: 1048576,
      outputLimit: 65536,
      modalities: {
        input: ["text", "image", "pdf", "video", "audio"],
        output: ["text"],
      },
      variants: {
        minimal: {
          thinkingConfig: { includeThoughts: true, thinkingLevel: "minimal" },
        },
        low: {
          thinkingConfig: { includeThoughts: true, thinkingLevel: "low" },
        },
        medium: {
          thinkingConfig: { includeThoughts: true, thinkingLevel: "medium" },
        },
        high: {
          thinkingConfig: { includeThoughts: true, thinkingLevel: "high" },
        },
      },
    },
    {
      id: "gemini-3-pro-preview",
      name: "Gemini 3 Pro Preview",
      contextLimit: 1048576,
      outputLimit: 65536,
      modalities: {
        input: ["text", "image", "pdf", "video", "audio"],
        output: ["text"],
      },
      variants: {
        low: {
          thinkingConfig: { includeThoughts: true, thinkingLevel: "low" },
        },
        high: {
          thinkingConfig: { includeThoughts: true, thinkingLevel: "high" },
        },
      },
    },
  ],
  "@ai-sdk/openai": [
    {
      id: "gpt-5",
      name: "GPT-5",
      contextLimit: 400000,
      outputLimit: 128000,
      modalities: { input: ["text", "image"], output: ["text"] },
      variants: {
        low: {
          reasoningEffort: "low",
          reasoningSummary: "auto",
          textVerbosity: "low",
        },
        medium: {
          reasoningEffort: "medium",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        high: {
          reasoningEffort: "high",
          reasoningSummary: "auto",
          textVerbosity: "high",
        },
      },
    },
    {
      id: "gpt-5.1",
      name: "GPT-5.1",
      contextLimit: 400000,
      outputLimit: 272000,
      modalities: { input: ["text", "image"], output: ["text"] },
      variants: {
        low: {
          reasoningEffort: "low",
          reasoningSummary: "auto",
          textVerbosity: "low",
        },
        medium: {
          reasoningEffort: "medium",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        high: {
          reasoningEffort: "high",
          reasoningSummary: "auto",
          textVerbosity: "high",
        },
      },
    },
    {
      id: "gpt-5.1-codex",
      name: "GPT-5.1 Codex",
      contextLimit: 400000,
      outputLimit: 128000,
      modalities: { input: ["text", "image"], output: ["text"] },
      options: { include: ["reasoning.encrypted_content"], store: false },
      variants: {
        low: {
          reasoningEffort: "low",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        medium: {
          reasoningEffort: "medium",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        high: {
          reasoningEffort: "high",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
      },
    },
    {
      id: "gpt-5.1-codex-max",
      name: "GPT-5.1 Codex Max",
      contextLimit: 400000,
      outputLimit: 128000,
      modalities: { input: ["text", "image"], output: ["text"] },
      options: { include: ["reasoning.encrypted_content"], store: false },
      variants: {
        low: {
          reasoningEffort: "low",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        medium: {
          reasoningEffort: "medium",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        high: {
          reasoningEffort: "high",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        xhigh: {
          reasoningEffort: "xhigh",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
      },
    },
    {
      id: "gpt-5.2",
      name: "GPT-5.2",
      contextLimit: 400000,
      outputLimit: 128000,
      modalities: { input: ["text", "image"], output: ["text"] },
      variants: {
        low: {
          reasoningEffort: "low",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        medium: {
          reasoningEffort: "medium",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        high: {
          reasoningEffort: "high",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        xhigh: {
          reasoningEffort: "xhigh",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
      },
    },
    {
      id: "gpt-5.2-codex",
      name: "GPT-5.2 Codex",
      contextLimit: 400000,
      outputLimit: 128000,
      modalities: { input: ["text", "image"], output: ["text"] },
      options: { include: ["reasoning.encrypted_content"], store: false },
      variants: {
        low: {
          reasoningEffort: "low",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        medium: {
          reasoningEffort: "medium",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        high: {
          reasoningEffort: "high",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        xhigh: {
          reasoningEffort: "xhigh",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
      },
    },
    {
      id: "gpt-5.3-codex",
      name: "GPT-5.3 Codex",
      contextLimit: 400000,
      outputLimit: 128000,
      modalities: { input: ["text", "image"], output: ["text"] },
      options: { include: ["reasoning.encrypted_content"], store: false },
      variants: {
        low: {
          reasoningEffort: "low",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        medium: {
          reasoningEffort: "medium",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        high: {
          reasoningEffort: "high",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
        xhigh: {
          reasoningEffort: "xhigh",
          reasoningSummary: "auto",
          textVerbosity: "medium",
        },
      },
    },
  ],
  "@ai-sdk/anthropic": [
    {
      id: "claude-sonnet-4-5-20250929",
      name: "Claude Sonnet 4.5",
      contextLimit: 200000,
      outputLimit: 64000,
      modalities: { input: ["text", "image", "pdf"], output: ["text"] },
      variants: {
        low: { effort: "low" },
        medium: { effort: "medium" },
        high: { effort: "high" },
      },
    },
    {
      id: "claude-opus-4-5-20251101",
      name: "Claude Opus 4.5",
      contextLimit: 200000,
      outputLimit: 64000,
      modalities: { input: ["text", "image", "pdf"], output: ["text"] },
      variants: {
        low: { thinking: { budgetTokens: 5000, type: "enabled" } },
        medium: { thinking: { budgetTokens: 13000, type: "enabled" } },
        high: { thinking: { budgetTokens: 18000, type: "enabled" } },
      },
    },
    {
      id: "claude-opus-4-6",
      name: "Claude Opus 4.6",
      contextLimit: 1000000,
      outputLimit: 128000,
      modalities: { input: ["text", "image", "pdf"], output: ["text"] },
      variants: {
        low: { effort: "low" },
        medium: { effort: "medium" },
        high: { effort: "high" },
        max: { effort: "max" },
      },
    },
    {
      id: "claude-haiku-4-5-20251001",
      name: "Claude Haiku 4.5",
      contextLimit: 200000,
      outputLimit: 64000,
      modalities: { input: ["text", "image", "pdf"], output: ["text"] },
    },
    {
      id: "gemini-claude-opus-4-5-thinking",
      name: "Antigravity - Claude Opus 4.5",
      contextLimit: 200000,
      outputLimit: 64000,
      modalities: { input: ["text", "image", "pdf"], output: ["text"] },
      variants: {
        low: { effort: "low" },
        medium: { effort: "medium" },
        high: { effort: "high" },
      },
    },
    {
      id: "gemini-claude-sonnet-4-5-thinking",
      name: "Antigravity - Claude Sonnet 4.5",
      contextLimit: 200000,
      outputLimit: 64000,
      modalities: { input: ["text", "image", "pdf"], output: ["text"] },
      variants: {
        low: { thinking: { budgetTokens: 5000, type: "enabled" } },
        medium: { thinking: { budgetTokens: 13000, type: "enabled" } },
        high: { thinking: { budgetTokens: 18000, type: "enabled" } },
      },
    },
  ],
};

/**
 * Look up preset metadata for a model by npm package and model ID.
 * Returns enrichment fields (options, limit, modalities) that can be
 * merged into a model definition when the user's config doesn't already
 * provide them.
 */
export function getPresetModelDefaults(
  npm: string,
  modelId: string,
): PresetModelVariant | undefined {
  const models = OPENCODE_PRESET_MODEL_VARIANTS[npm];
  if (!models) return undefined;
  return models.find((m) => m.id === modelId);
}

export const opencodeProviderPresets: OpenCodeProviderPreset[] = [
  {
    name: "DeepSeek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      options: {
        baseURL: "https://api.deepseek.com/v1",
        apiKey: "",
      },
      models: {
        "deepseek-chat": { name: "DeepSeek V3.2" },
        "deepseek-reasoner": { name: "DeepSeek R1" },
      },
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
  },
  {
    name: "Zhipu GLM",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "Zhipu GLM",
      options: {
        baseURL: "https://open.bigmodel.cn/api/paas/v4",
        apiKey: "",
      },
      models: {
        "glm-4.7": { name: "GLM-4.7" },
      },
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "zhipu",
    icon: "zhipu",
    iconColor: "#0F62FE",
    templateValues: {
      baseURL: {
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
  },
  {
    name: "Zhipu GLM en",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe?ic=8JVLJQFSKB",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "Zhipu GLM en",
      options: {
        baseURL: "https://api.z.ai/v1",
        apiKey: "",
      },
      models: {
        "glm-4.7": { name: "GLM-4.7" },
      },
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "zhipu",
    icon: "zhipu",
    iconColor: "#0F62FE",
    templateValues: {
      baseURL: {
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
  },
  {
    name: "Bailian",
    websiteUrl: "https://bailian.console.aliyun.com",
    apiKeyUrl: "https://bailian.console.aliyun.com/#/api-key",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "Bailian",
      options: {
        baseURL: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        apiKey: "",
      },
      models: {},
    },
    category: "cn_official",
    icon: "bailian",
    iconColor: "#624AFF",
    templateValues: {
      baseURL: {
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
  },
  {
    name: "Kimi k2.5",
    websiteUrl: "https://platform.moonshot.cn/console",
    apiKeyUrl: "https://platform.moonshot.cn/console/api-keys",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "Kimi k2.5",
      options: {
        baseURL: "https://api.moonshot.cn/v1",
        apiKey: "",
      },
      models: {
        "kimi-k2.5": { name: "Kimi K2.5" },
      },
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
    templateValues: {
      baseURL: {
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
  },
  {
    name: "Kimi For Coding",
    websiteUrl: "https://www.kimi.com/coding/docs/",
    apiKeyUrl: "https://platform.moonshot.cn/console/api-keys",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "Kimi For Coding",
      options: {
        baseURL: "https://api.kimi.com/v1",
        apiKey: "",
      },
      models: {
        "kimi-for-coding": { name: "Kimi For Coding" },
      },
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
    templateValues: {
      baseURL: {
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
  },
  {
    name: "ModelScope",
    websiteUrl: "https://modelscope.cn",
    apiKeyUrl: "https://modelscope.cn/my/myaccesstoken",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "ModelScope",
      options: {
        baseURL: "https://api-inference.modelscope.cn/v1",
        apiKey: "",
      },
      models: {
        "ZhipuAI/GLM-4.7": { name: "GLM-4.7" },
      },
    },
    category: "aggregator",
    icon: "modelscope",
    iconColor: "#624AFF",
    templateValues: {
      baseURL: {
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
  },
  {
    name: "KAT-Coder",
    websiteUrl: "https://console.streamlake.ai",
    apiKeyUrl: "https://console.streamlake.ai/console/api-key",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "KAT-Coder",
      options: {
        baseURL:
          "https://vanchin.streamlake.ai/api/gateway/v1/endpoints/${ENDPOINT_ID}/openai",
        apiKey: "",
      },
      models: {
        "KAT-Coder-Pro": { name: "KAT-Coder Pro" },
      },
    },
    category: "cn_official",
    templateValues: {
      baseURL: {
        label: "Base URL",
        placeholder:
          "https://vanchin.streamlake.ai/api/gateway/v1/endpoints/${ENDPOINT_ID}/openai",
        defaultValue:
          "https://vanchin.streamlake.ai/api/gateway/v1/endpoints/${ENDPOINT_ID}/openai",
        editorValue: "",
      },
      ENDPOINT_ID: {
        label: "Vanchin Endpoint ID",
        placeholder: "ep-xxx-xxx",
        defaultValue: "",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    icon: "catcoder",
  },
  {
    name: "Longcat",
    websiteUrl: "https://longcat.chat/platform",
    apiKeyUrl: "https://longcat.chat/platform/api_keys",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "Longcat",
      options: {
        baseURL: "https://api.longcat.chat/v1",
        apiKey: "",
      },
      models: {
        "LongCat-Flash-Chat": { name: "LongCat Flash Chat" },
      },
    },
    category: "cn_official",
    icon: "longcat",
    iconColor: "#29E154",
    templateValues: {
      baseURL: {
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
  },
  {
    name: "MiniMax",
    websiteUrl: "https://platform.minimaxi.com",
    apiKeyUrl: "https://platform.minimaxi.com/subscribe/coding-plan",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "MiniMax",
      options: {
        baseURL: "https://api.minimaxi.com/v1",
        apiKey: "",
      },
      models: {
        "MiniMax-M2.1": { name: "MiniMax M2.1" },
      },
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
  },
  {
    name: "MiniMax en",
    websiteUrl: "https://platform.minimax.io",
    apiKeyUrl: "https://platform.minimax.io/subscribe/coding-plan",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "MiniMax en",
      options: {
        baseURL: "https://api.minimax.io/v1",
        apiKey: "",
      },
      models: {
        "MiniMax-M2.1": { name: "MiniMax M2.1" },
      },
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
  },
  {
    name: "DouBaoSeed",
    websiteUrl: "https://www.volcengine.com/product/doubao",
    apiKeyUrl: "https://www.volcengine.com/product/doubao",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "DouBaoSeed",
      options: {
        baseURL: "https://ark.cn-beijing.volces.com/api/v3",
        apiKey: "",
      },
      models: {
        "doubao-seed-code-preview-latest": { name: "Doubao Seed Code Preview" },
      },
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
  },
  {
    name: "BaiLing",
    websiteUrl: "https://alipaytbox.yuque.com/sxs0ba/ling/get_started",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "BaiLing",
      options: {
        baseURL: "https://api.tbox.cn/v1",
        apiKey: "",
      },
      models: {
        "Ling-1T": { name: "Ling 1T" },
      },
    },
    category: "cn_official",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
  },
  {
    name: "Xiaomi MiMo",
    websiteUrl: "https://platform.xiaomimimo.com",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/api-keys",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "Xiaomi MiMo",
      options: {
        baseURL: "https://api.xiaomimimo.com/v1",
        apiKey: "",
      },
      models: {
        "mimo-v2-flash": { name: "MiMo V2 Flash" },
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

  {
    name: "AiHubMix",
    websiteUrl: "https://aihubmix.com",
    apiKeyUrl: "https://aihubmix.com",
    settingsConfig: {
      npm: "@ai-sdk/anthropic",
      name: "AiHubMix",
      options: {
        baseURL: "https://aihubmix.com/v1",
        apiKey: "",
      },
      models: {
        "claude-sonnet-4-5-20250929": { name: "Claude Sonnet 4.5" },
        "claude-opus-4-5-20251101": { name: "Claude Opus 4.5" },
      },
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
  },
  {
    name: "DMXAPI",
    websiteUrl: "https://www.dmxapi.cn",
    apiKeyUrl: "https://www.dmxapi.cn",
    settingsConfig: {
      npm: "@ai-sdk/anthropic",
      name: "DMXAPI",
      options: {
        baseURL: "https://www.dmxapi.cn/v1",
        apiKey: "",
      },
      models: {
        "claude-sonnet-4-5-20250929": { name: "Claude Sonnet 4.5" },
        "claude-opus-4-5-20251101": { name: "Claude Opus 4.5" },
      },
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
  },
  {
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      npm: "@ai-sdk/anthropic",
      name: "OpenRouter",
      options: {
        baseURL: "https://openrouter.ai/api/v1",
        apiKey: "",
      },
      models: {
        "anthropic/claude-sonnet-4.5": { name: "Claude Sonnet 4.5" },
        "anthropic/claude-opus-4.5": { name: "Claude Opus 4.5" },
      },
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
  },
  {
    name: "Nvidia",
    websiteUrl: "https://build.nvidia.com",
    apiKeyUrl: "https://build.nvidia.com/settings/api-keys",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "Nvidia",
      options: {
        baseURL: "https://integrate.api.nvidia.com/v1",
        apiKey: "",
      },
      models: {
        "moonshotai/kimi-k2.5": { name: "Kimi K2.5" },
      },
    },
    category: "aggregator",
    icon: "nvidia",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
  },

  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.com",
    apiKeyUrl: "https://www.packyapi.com/register?aff=cc-switch",
    settingsConfig: {
      npm: "@ai-sdk/anthropic",
      name: "PackyCode",
      options: {
        baseURL: "https://www.packyapi.com/v1",
        apiKey: "",
      },
      models: {
        "claude-sonnet-4-5-20250929": { name: "Claude Sonnet 4.5" },
        "claude-opus-4-5-20251101": { name: "Claude Opus 4.5" },
      },
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
  },
  {
    name: "Cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    settingsConfig: {
      npm: "@ai-sdk/anthropic",
      name: "Cubence",
      options: {
        baseURL: "https://api.cubence.com/v1",
        apiKey: "",
      },
      models: {
        "claude-sonnet-4-5-20250929": { name: "Claude Sonnet 4.5" },
        "claude-opus-4-5-20251101": { name: "Claude Opus 4.5" },
      },
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
  },
  {
    name: "AIGoCode",
    websiteUrl: "https://aigocode.com",
    apiKeyUrl: "https://aigocode.com/invite/CC-SWITCH",
    settingsConfig: {
      npm: "@ai-sdk/anthropic",
      name: "AIGoCode",
      options: {
        baseURL: "https://api.aigocode.com",
        apiKey: "",
      },
      models: {
        "claude-sonnet-4-5-20250929": { name: "Claude Sonnet 4.5" },
        "claude-opus-4-5-20251101": { name: "Claude Opus 4.5" },
      },
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
  },
  {
    name: "RightCode",
    websiteUrl: "https://www.right.codes",
    apiKeyUrl: "https://www.right.codes/register?aff=CCSWITCH",
    settingsConfig: {
      npm: "@ai-sdk/openai",
      name: "RightCode",
      options: {
        baseURL: "https://right.codes/codex/v1",
        apiKey: "",
      },
      models: {
        "gpt-5.2": { name: "GPT-5.2" },
        "gpt-5.2-codex": {
          name: "GPT-5.2 Codex",
          options: { include: ["reasoning.encrypted_content"], store: false },
        },
      },
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
  },
  {
    name: "AICodeMirror",
    websiteUrl: "https://www.aicodemirror.com",
    apiKeyUrl: "https://www.aicodemirror.com/register?invitecode=9915W3",
    settingsConfig: {
      npm: "@ai-sdk/anthropic",
      name: "AICodeMirror",
      options: {
        baseURL: "https://api.aicodemirror.com/api/claudecode",
        apiKey: "",
      },
      models: {
        "claude-sonnet-4.5": { name: "Claude Sonnet 4.5" },
        "claude-opus-4.5": { name: "Claude Opus 4.5" },
      },
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
  },

  {
    name: "OpenAI Compatible",
    websiteUrl: "",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      options: {
        baseURL: "",
        apiKey: "",
      },
      models: {},
    },
    category: "custom",
    isCustomTemplate: true,
    icon: "generic",
    iconColor: "#6B7280",
    templateValues: {
      baseURL: {
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

  {
    name: "Oh My OpenCode",
    websiteUrl: "https://github.com/code-yeongyu/oh-my-opencode",
    settingsConfig: {
      npm: "",
      options: {},
      models: {},
    },
    category: "omo" as ProviderCategory,
    icon: "opencode",
    iconColor: "#8B5CF6",
    isCustomTemplate: true,
  },
];
