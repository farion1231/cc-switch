/**
 * OpenCode 预设供应商配置模板
 * OpenCode 使用 AI SDK npm 包，配置结构与其他应用不同
 */
import type { ProviderCategory, OpenCodeProviderConfig } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

export interface OpenCodeProviderPreset {
  name: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  /** OpenCode settings_config 结构 */
  settingsConfig: OpenCodeProviderConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  /** 模板变量定义 */
  templateValues?: Record<string, TemplateValueConfig>;
  /** 视觉主题配置 */
  theme?: PresetTheme;
  /** 图标名称 */
  icon?: string;
  /** 图标颜色 */
  iconColor?: string;
  /** 标记为自定义模板（用于 UI 区分） */
  isCustomTemplate?: boolean;
}

/**
 * OpenCode npm 包选项（AI SDK 生态）
 */
export const opencodeNpmPackages = [
  { value: "@ai-sdk/openai", label: "OpenAI" },
  { value: "@ai-sdk/openai-compatible", label: "OpenAI Compatible" },
  { value: "@ai-sdk/anthropic", label: "Anthropic" },
  { value: "@ai-sdk/google", label: "Google (Gemini)" },
] as const;

/**
 * OpenCode 供应商预设列表
 */
export const opencodeProviderPresets: OpenCodeProviderPreset[] = [
  // ========== 国产官方 ==========
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
    name: "Qwen Coder",
    websiteUrl: "https://bailian.console.aliyun.com",
    apiKeyUrl: "https://bailian.console.aliyun.com/#/api-key",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      name: "Qwen Coder",
      options: {
        baseURL: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        apiKey: "",
      },
      models: {
        "qwen3-max": { name: "Qwen3 Max" },
      },
    },
    category: "cn_official",
    icon: "qwen",
    iconColor: "#FF6A00",
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

  // ========== 聚合网站 ==========
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

  // ========== 第三方合作伙伴 ==========
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
        baseURL: "https://api.aigocode.com/v1",
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
        "gpt-5.2-codex": { name: "GPT-5.2 Codex" },
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

  // ========== 自定义模板 ==========
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
];
