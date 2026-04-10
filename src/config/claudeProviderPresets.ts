/**
 * 预设供应商配置模板
 */
import { ProviderCategory } from "../types";

export interface TemplateValueConfig {
  label: string;
  placeholder: string;
  defaultValue?: string;
  editorValue: string;
}

/**
 * 预设供应商的视觉主题配置
 */
export interface PresetTheme {
  /** 图标类型：'claude' | 'codex' | 'gemini' | 'generic' */
  icon?: "claude" | "codex" | "gemini" | "generic";
  /** 背景色（选中状态），支持 Tailwind 类名或 hex 颜色 */
  backgroundColor?: string;
  /** 文字色（选中状态），支持 Tailwind 类名或 hex 颜色 */
  textColor?: string;
}

export interface ProviderPreset {
  name: string;
  nameKey?: string; // i18n key for localized display name
  websiteUrl: string;
  // 新增：第三方/聚合等可单独配置获取 API Key 的链接
  apiKeyUrl?: string;
  settingsConfig: object;
  isOfficial?: boolean; // 标识是否为官方预设
  isPartner?: boolean; // 标识是否为商业合作伙伴
  partnerPromotionKey?: string; // 合作伙伴促销信息的 i18n key
  category?: ProviderCategory; // 新增：分类
  // 新增：指定该预设所使用的 API Key 字段名（默认 ANTHROPIC_AUTH_TOKEN）
  apiKeyField?: "ANTHROPIC_AUTH_TOKEN" | "ANTHROPIC_API_KEY";
  // 新增：模板变量定义，用于动态替换配置中的值
  templateValues?: Record<string, TemplateValueConfig>; // editorValue 存储编辑器中的实时输入值
  // 新增：请求地址候选列表（用于地址管理/测速）
  endpointCandidates?: string[];
  // 新增：视觉主题配置
  theme?: PresetTheme;
  // 图标配置
  icon?: string; // 图标名称
  iconColor?: string; // 图标颜色

  // Claude API 格式（仅 Claude 供应商使用）
  // - "anthropic" (默认): Anthropic Messages API 格式，直接透传
  // - "openai_chat": OpenAI Chat Completions 格式，需要格式转换
  // - "openai_responses": OpenAI Responses API 格式，需要格式转换
  apiFormat?: "anthropic" | "openai_chat" | "openai_responses";

  // 供应商类型标识（用于特殊供应商检测）
  // - "github_copilot": GitHub Copilot 供应商（需要 OAuth 认证）
  // - "codex_oauth": OpenAI Codex via ChatGPT Plus/Pro 反代（需要 OAuth 认证）
  providerType?: "github_copilot" | "codex_oauth";

  // 是否需要 OAuth 认证（而非 API Key）
  requiresOAuth?: boolean;

  // 是否在 UI 中隐藏该预设（预设仍存在，仅不在列表中显示）
  hidden?: boolean;
}

export const providerPresets: ProviderPreset[] = [
  {
    name: "Claude Official",
    websiteUrl: "https://www.anthropic.com/claude-code",
    settingsConfig: {
      env: {},
    },
    isOfficial: true, // 明确标识为官方预设
    category: "official",
    theme: {
      icon: "claude",
      backgroundColor: "#D97757",
      textColor: "#FFFFFF",
    },
    icon: "anthropic",
    iconColor: "#D4915D",
  },
  {
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://router.shengsuanyun.com/api",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "shengsuanyun",
    icon: "shengsuanyun",
  },
  {
    name: "DeepSeek",
    websiteUrl: "https://platform.deepseek.com",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.deepseek.com/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "DeepSeek-V3.2",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "DeepSeek-V3.2",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "DeepSeek-V3.2",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "DeepSeek-V3.2",
      },
    },
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#1E88E5",
  },
  {
    name: "Zhipu GLM",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://open.bigmodel.cn/api/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "glm-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "glm-5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "glm-5",
      },
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
  },
  {
    name: "Zhipu GLM en",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe?ic=8JVLJQFSKB",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.z.ai/api/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "glm-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "glm-5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "glm-5",
      },
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
  },
  {
    name: "Bailian",
    websiteUrl: "https://bailian.console.aliyun.com",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://dashscope.aliyuncs.com/apps/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "cn_official",
    icon: "bailian",
    iconColor: "#624AFF",
  },
  {
    name: "Bailian For Coding",
    websiteUrl: "https://bailian.console.aliyun.com",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://coding.dashscope.aliyuncs.com/apps/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "cn_official",
    icon: "bailian",
    iconColor: "#624AFF",
  },
  {
    name: "Kimi",
    websiteUrl: "https://platform.moonshot.cn/console",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.moonshot.cn/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "kimi-k2.5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "kimi-k2.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "kimi-k2.5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "kimi-k2.5",
      },
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "Kimi For Coding",
    websiteUrl: "https://www.kimi.com/coding/docs/",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.kimi.com/coding/",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "StepFun",
    websiteUrl: "https://platform.stepfun.ai",
    apiKeyUrl: "https://platform.stepfun.ai/interface-key",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.stepfun.ai/v1",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "step-3.5-flash",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "step-3.5-flash",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "step-3.5-flash",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "step-3.5-flash",
      },
    },
    category: "cn_official",
    endpointCandidates: ["https://api.stepfun.ai/v1"],
    apiFormat: "openai_chat",
    icon: "stepfun",
    iconColor: "#005AFF",
  },
  {
    name: "ModelScope",
    websiteUrl: "https://modelscope.cn",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api-inference.modelscope.cn",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "ZhipuAI/GLM-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "ZhipuAI/GLM-5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "ZhipuAI/GLM-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "ZhipuAI/GLM-5",
      },
    },
    category: "aggregator",
    icon: "modelscope",
    iconColor: "#624AFF",
  },
  {
    name: "KAT-Coder",
    websiteUrl: "https://console.streamlake.ai",
    apiKeyUrl: "https://console.streamlake.ai/console/api-key",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://vanchin.streamlake.ai/api/gateway/v1/endpoints/${ENDPOINT_ID}/claude-code-proxy",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "KAT-Coder-Pro V1",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "KAT-Coder-Air V1",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "KAT-Coder-Pro V1",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "KAT-Coder-Pro V1",
      },
    },
    category: "cn_official",
    templateValues: {
      ENDPOINT_ID: {
        label: "Vanchin Endpoint ID",
        placeholder: "ep-xxx-xxx",
        defaultValue: "",
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
      env: {
        ANTHROPIC_BASE_URL: "https://api.longcat.chat/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "LongCat-Flash-Chat",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "LongCat-Flash-Chat",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "LongCat-Flash-Chat",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "LongCat-Flash-Chat",
        CLAUDE_CODE_MAX_OUTPUT_TOKENS: "6000",
        CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: 1,
      },
    },
    category: "cn_official",
    icon: "longcat",
    iconColor: "#29E154",
  },
  {
    name: "MiniMax",
    websiteUrl: "https://platform.minimaxi.com",
    apiKeyUrl: "https://platform.minimaxi.com/subscribe/coding-plan",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.minimaxi.com/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        API_TIMEOUT_MS: "3000000",
        CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: 1,
        ANTHROPIC_MODEL: "MiniMax-M2.7",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "MiniMax-M2.7",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "MiniMax-M2.7",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "MiniMax-M2.7",
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
  },
  {
    name: "MiniMax en",
    websiteUrl: "https://platform.minimax.io",
    apiKeyUrl: "https://platform.minimax.io/subscribe/coding-plan",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.minimax.io/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        API_TIMEOUT_MS: "3000000",
        CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: 1,
        ANTHROPIC_MODEL: "MiniMax-M2.7",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "MiniMax-M2.7",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "MiniMax-M2.7",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "MiniMax-M2.7",
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
  },
  {
    name: "DouBaoSeed",
    websiteUrl: "https://www.volcengine.com/product/doubao",
    apiKeyUrl: "https://www.volcengine.com/product/doubao",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://ark.cn-beijing.volces.com/api/coding",
        ANTHROPIC_AUTH_TOKEN: "",
        API_TIMEOUT_MS: "3000000",
        ANTHROPIC_MODEL: "doubao-seed-2-0-code-preview-latest",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "doubao-seed-2-0-code-preview-latest",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "doubao-seed-2-0-code-preview-latest",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "doubao-seed-2-0-code-preview-latest",
      },
    },
    category: "cn_official",
    icon: "doubao",
    iconColor: "#3370FF",
  },
  {
    name: "BaiLing",
    websiteUrl: "https://alipaytbox.yuque.com/sxs0ba/ling/get_started",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.tbox.cn/api/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "Ling-2.5-1T",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "Ling-2.5-1T",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "Ling-2.5-1T",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "Ling-2.5-1T",
      },
    },
    category: "cn_official",
  },
  {
    name: "AiHubMix",
    websiteUrl: "https://aihubmix.com",
    apiKeyUrl: "https://aihubmix.com",
    // 说明：该供应商使用 ANTHROPIC_API_KEY（而非 ANTHROPIC_AUTH_TOKEN）
    apiKeyField: "ANTHROPIC_API_KEY",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://aihubmix.com",
        ANTHROPIC_API_KEY: "",
      },
    },
    // 请求地址候选（用于地址管理/测速），用户可自行选择/覆盖
    endpointCandidates: ["https://aihubmix.com", "https://api.aihubmix.com"],
    category: "aggregator",
    icon: "aihubmix",
    iconColor: "#006FFB",
  },
  {
    name: "SiliconFlow",
    websiteUrl: "https://siliconflow.cn",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/drGuwc9k",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.siliconflow.cn",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "Pro/MiniMaxAI/MiniMax-M2.7",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "Pro/MiniMaxAI/MiniMax-M2.7",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "Pro/MiniMaxAI/MiniMax-M2.7",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "Pro/MiniMaxAI/MiniMax-M2.7",
      },
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "siliconflow",
    icon: "siliconflow",
    iconColor: "#6E29F6",
  },
  {
    name: "SiliconFlow en",
    websiteUrl: "https://siliconflow.com",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/drGuwc9k",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.siliconflow.com",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "MiniMaxAI/MiniMax-M2.7",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "MiniMaxAI/MiniMax-M2.7",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "MiniMaxAI/MiniMax-M2.7",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "MiniMaxAI/MiniMax-M2.7",
      },
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "siliconflow",
    icon: "siliconflow",
    iconColor: "#000000",
  },
  {
    name: "DMXAPI",
    websiteUrl: "https://www.dmxapi.cn",
    apiKeyUrl: "https://www.dmxapi.cn",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://www.dmxapi.cn",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    // 请求地址候选（用于地址管理/测速），用户可自行选择/覆盖
    endpointCandidates: ["https://www.dmxapi.cn", "https://api.dmxapi.cn"],
    category: "aggregator",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "dmxapi", // 促销信息 i18n key
  },
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.com",
    apiKeyUrl: "https://www.packyapi.com/register?aff=cc-switch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://www.packyapi.com",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    // 请求地址候选（用于地址管理/测速）
    endpointCandidates: [
      "https://www.packyapi.com",
      "https://api-slb.packyapi.com",
    ],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "packycode", // 促销信息 i18n key
    icon: "packycode",
  },
  {
    name: "Cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.cubence.com",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: [
      "https://api.cubence.com",
      "https://api-cf.cubence.com",
      "https://api-dmit.cubence.com",
      "https://api-bwg.cubence.com",
    ],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "cubence", // 促销信息 i18n key
    icon: "cubence",
    iconColor: "#000000",
  },
  {
    name: "AIGoCode",
    websiteUrl: "https://aigocode.com",
    apiKeyUrl: "https://aigocode.com/invite/CC-SWITCH",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.aigocode.com",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    // 请求地址候选（用于地址管理/测速）
    endpointCandidates: ["https://api.aigocode.com"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "aigocode", // 促销信息 i18n key
    icon: "aigocode",
    iconColor: "#5B7FFF",
  },
  {
    name: "RightCode",
    websiteUrl: "https://www.right.codes",
    apiKeyUrl: "https://www.right.codes/register?aff=CCSWITCH",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://www.right.codes/claude",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "rightcode",
    icon: "rc",
    iconColor: "#E96B2C",
  },
  {
    name: "AICodeMirror",
    websiteUrl: "https://www.aicodemirror.com",
    apiKeyUrl: "https://www.aicodemirror.com/register?invitecode=9915W3",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.aicodemirror.com/api/claudecode",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: [
      "https://api.aicodemirror.com/api/claudecode",
      "https://api.claudecode.net.cn/api/claudecode",
    ],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "aicodemirror", // 促销信息 i18n key
    icon: "aicodemirror",
    iconColor: "#000000",
  },
  {
    name: "AICoding",
    websiteUrl: "https://aicoding.sh",
    apiKeyUrl: "https://aicoding.sh/i/CCSWITCH",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.aicoding.sh",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://api.aicoding.sh"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "aicoding", // 促销信息 i18n key
    icon: "aicoding",
    iconColor: "#000000",
  },
  {
    name: "CrazyRouter",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://crazyrouter.com",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://crazyrouter.com"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "crazyrouter", // 促销信息 i18n key
    icon: "crazyrouter",
    iconColor: "#000000",
  },
  {
    name: "SSSAiCode",
    websiteUrl: "https://www.sssaicode.com",
    apiKeyUrl: "https://www.sssaicode.com/register?ref=DCP0SM",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://node-hk.sssaicode.com/api",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: [
      "https://node-hk.sssaicode.com/api",
      "https://claude2.sssaicode.com/api",
      "https://anti.sssaicode.com/api",
    ],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "sssaicode", // 促销信息 i18n key
    icon: "sssaicode",
    iconColor: "#000000",
  },
  {
    name: "Compshare",
    nameKey: "providerForm.presets.ucloud",
    websiteUrl: "https://www.compshare.cn",
    apiKeyUrl:
      "https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.modelverse.cn",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://api.modelverse.cn"],
    category: "aggregator",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "ucloud", // 促销信息 i18n key
    icon: "ucloud",
    iconColor: "#000000",
  },
  {
    name: "Micu",
    websiteUrl: "https://www.openclaudecode.cn",
    apiKeyUrl: "https://www.openclaudecode.cn/register?aff=aOYQ",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://www.openclaudecode.cn",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://www.openclaudecode.cn"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "micu", // 促销信息 i18n key
    icon: "micu",
    iconColor: "#000000",
  },
  {
    name: "X-Code API",
    websiteUrl: "https://x-code.cc",
    apiKeyUrl: "https://x-code.cc",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://x-code.cc",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://x-code.cc"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "x-code", // 促销信息 i18n key
    icon: "xcode",
    iconColor: "#000000",
  },
  {
    name: "CTok.ai",
    websiteUrl: "https://ctok.ai",
    apiKeyUrl: "https://ctok.ai",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.ctok.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "ctok", // 促销信息 i18n key
    icon: "ctok",
    iconColor: "#000000",
  },
  {
    name: "DDSHub",
    websiteUrl: "https://www.ddshub.cc",
    apiKeyUrl: "https://ddshub.short.gy/ccswitch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://www.ddshub.cc",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "ddshub", // 促销信息 i18n key
    icon: "dds",
    iconColor: "#000000",
  },
  {
    name: "E-FlowCode",
    websiteUrl: "https://e-flowcode.cc",
    apiKeyUrl: "https://e-flowcode.cc",
    settingsConfig: {
      effortLevel: "high",
      env: {
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_BASE_URL: "https://e-flowcode.cc",
      },
      enabledPlugins: {
        "superpowers@superpowers-marketplace": true,
      },
      includeCoAuthoredBy: false,
      ENABLE_TOOL_SEARCH: true,
      skipWebFetchPreflight: true,
    },
    category: "third_party",
    endpointCandidates: ["https://e-flowcode.cc"],
    icon: "eflowcode",
    iconColor: "#000000",
  },
  {
    name: "LionCCAPI",
    websiteUrl: "https://vibecodingapi.ai",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://vibecodingapi.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "lionccapi",
    icon: "lioncc",
  },
  {
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://openrouter.ai/api",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "anthropic/claude-sonnet-4.6",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "anthropic/claude-haiku-4.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "anthropic/claude-sonnet-4.6",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "anthropic/claude-opus-4.6",
      },
    },
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6566F1",
  },
  {
    name: "TheRouter",
    websiteUrl: "https://therouter.ai",
    apiKeyUrl: "https://dashboard.therouter.ai",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.therouter.ai",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_API_KEY: "",
        ANTHROPIC_MODEL: "anthropic/claude-sonnet-4.6",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "anthropic/claude-haiku-4.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "anthropic/claude-sonnet-4.6",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "anthropic/claude-opus-4.6",
      },
    },
    category: "aggregator",
    endpointCandidates: ["https://api.therouter.ai"],
  },
  {
    name: "Novita AI",
    websiteUrl: "https://novita.ai",
    apiKeyUrl: "https://novita.ai",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.novita.ai/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "zai-org/glm-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "zai-org/glm-5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "zai-org/glm-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "zai-org/glm-5",
      },
    },
    category: "aggregator",
    endpointCandidates: ["https://api.novita.ai/anthropic"],
    icon: "novita",
    iconColor: "#000000",
  },
  {
    name: "GitHub Copilot",
    websiteUrl: "https://github.com/features/copilot",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.githubcopilot.com",
        ANTHROPIC_MODEL: "claude-opus-4.6",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-haiku-4.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-4.6",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-4.6",
      },
    },
    category: "third_party",
    apiFormat: "openai_chat",
    providerType: "github_copilot",
    requiresOAuth: true,
    icon: "github",
    iconColor: "#000000",
  },
  {
    name: "Codex",
    websiteUrl: "https://openai.com/chatgpt/pricing",
    settingsConfig: {
      env: {
        // base_url 由代理后端强制重写为 chatgpt.com/backend-api/codex
        // 用户无需配置
        ANTHROPIC_BASE_URL: "https://chatgpt.com/backend-api/codex",
        ANTHROPIC_MODEL: "gpt-5.4",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "gpt-5.4-mini",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "gpt-5.4",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "gpt-5.4",
      },
    },
    category: "third_party",
    apiFormat: "openai_responses",
    providerType: "codex_oauth",
    requiresOAuth: true,
    icon: "openai",
    iconColor: "#000000",
  },
  {
    name: "Nvidia",
    websiteUrl: "https://build.nvidia.com",
    apiKeyUrl: "https://build.nvidia.com/settings/api-keys",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://integrate.api.nvidia.com",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "moonshotai/kimi-k2.5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "moonshotai/kimi-k2.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "moonshotai/kimi-k2.5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "moonshotai/kimi-k2.5",
      },
    },
    category: "aggregator",
    apiFormat: "openai_chat",
    icon: "nvidia",
    iconColor: "#000000",
  },
  {
    name: "PIPELLM",
    websiteUrl: "https://www.pipellm.ai",
    apiKeyUrl: "https://code.pipellm.ai/login?ref=uvw650za",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://cc-api.pipellm.ai",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "claude-opus-4-6",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-haiku-4-5-20251001",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-4-6",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-4-6",
      },
      includeCoAuthoredBy: false,
    },
    category: "aggregator",
    icon: "pipellm",
  },
  {
    name: "Xiaomi MiMo",
    websiteUrl: "https://platform.xiaomimimo.com",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/api-keys",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.xiaomimimo.com/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "mimo-v2-pro",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "mimo-v2-pro",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "mimo-v2-pro",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "mimo-v2-pro",
      },
    },
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
  },
  {
    name: "AWS Bedrock (AKSK)",
    websiteUrl: "https://aws.amazon.com/bedrock/",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://bedrock-runtime.${AWS_REGION}.amazonaws.com",
        AWS_ACCESS_KEY_ID: "${AWS_ACCESS_KEY_ID}",
        AWS_SECRET_ACCESS_KEY: "${AWS_SECRET_ACCESS_KEY}",
        AWS_REGION: "${AWS_REGION}",
        ANTHROPIC_MODEL: "global.anthropic.claude-opus-4-6-v1",
        ANTHROPIC_DEFAULT_HAIKU_MODEL:
          "global.anthropic.claude-haiku-4-5-20251001-v1:0",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "global.anthropic.claude-sonnet-4-6",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "global.anthropic.claude-opus-4-6-v1",
        CLAUDE_CODE_USE_BEDROCK: "1",
      },
    },
    category: "cloud_provider",
    templateValues: {
      AWS_REGION: {
        label: "AWS Region",
        placeholder: "us-west-2",
        editorValue: "us-west-2",
      },
      AWS_ACCESS_KEY_ID: {
        label: "Access Key ID",
        placeholder: "AKIA...",
        editorValue: "",
      },
      AWS_SECRET_ACCESS_KEY: {
        label: "Secret Access Key",
        placeholder: "your-secret-key",
        editorValue: "",
      },
    },
    icon: "aws",
    iconColor: "#FF9900",
  },
  {
    name: "AWS Bedrock (API Key)",
    websiteUrl: "https://aws.amazon.com/bedrock/",
    settingsConfig: {
      apiKey: "",
      env: {
        ANTHROPIC_BASE_URL:
          "https://bedrock-runtime.${AWS_REGION}.amazonaws.com",
        AWS_REGION: "${AWS_REGION}",
        ANTHROPIC_MODEL: "global.anthropic.claude-opus-4-6-v1",
        ANTHROPIC_DEFAULT_HAIKU_MODEL:
          "global.anthropic.claude-haiku-4-5-20251001-v1:0",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "global.anthropic.claude-sonnet-4-6",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "global.anthropic.claude-opus-4-6-v1",
        CLAUDE_CODE_USE_BEDROCK: "1",
      },
    },
    category: "cloud_provider",
    templateValues: {
      AWS_REGION: {
        label: "AWS Region",
        placeholder: "us-west-2",
        editorValue: "us-west-2",
      },
    },
    icon: "aws",
    iconColor: "#FF9900",
  },
];
