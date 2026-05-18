import type { ProviderCategory } from "@/types";

/**
 * Gemini 预设供应商的视觉主题配置
 */
export interface GeminiPresetTheme {
  /** 图标类型：'gemini' | 'generic' */
  icon?: "gemini" | "generic";
  /** 背景色（选中状态），支持 hex 颜色 */
  backgroundColor?: string;
  /** 文字色（选中状态），支持 hex 颜色 */
  textColor?: string;
}

export interface GeminiProviderPreset {
  name: string;
  nameKey?: string; // i18n key for localized display name
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: object;
  baseURL?: string;
  model?: string;
  description?: string;
  category?: ProviderCategory;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  endpointCandidates?: string[];
  theme?: GeminiPresetTheme;
  // 图标配置
  icon?: string; // 图标名称
  iconColor?: string; // 图标颜色
}

export const geminiProviderPresets: GeminiProviderPreset[] = [
  {
    name: "Google Official",
    websiteUrl: "https://ai.google.dev/",
    apiKeyUrl: "https://aistudio.google.com/apikey",
    settingsConfig: {
      env: {},
    },
    description: "Google 官方 Gemini API (OAuth)",
    category: "official",
    partnerPromotionKey: "google-official",
    theme: {
      icon: "gemini",
      backgroundColor: "#4285F4",
      textColor: "#FFFFFF",
    },
    icon: "gemini",
    iconColor: "#4285F4",
  },
  {
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://router.shengsuanyun.com/api",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    baseURL: "https://router.shengsuanyun.com/api",
    model: "gemini-3.1-pro",
    description: "Shengsuanyun",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "shengsuanyun",
    icon: "shengsuanyun",
  },
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.com",
    apiKeyUrl: "https://www.packyapi.com/register?aff=cc-switch",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://www.packyapi.com",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    baseURL: "https://www.packyapi.com",
    model: "gemini-3.1-pro",
    description: "PackyCode",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "packycode",
    endpointCandidates: [
      "https://api-slb.packyapi.com",
      "https://www.packyapi.com",
    ],
    icon: "packycode",
  },
  {
    name: "Cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.cubence.com",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    baseURL: "https://api.cubence.com",
    model: "gemini-3.1-pro",
    description: "Cubence",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "cubence",
    endpointCandidates: [
      "https://api.cubence.com/v1",
      "https://api-cf.cubence.com/v1",
      "https://api-dmit.cubence.com/v1",
      "https://api-bwg.cubence.com/v1",
    ],
    icon: "cubence",
    iconColor: "#000000",
  },
  {
    name: "AIGoCode",
    websiteUrl: "https://aigocode.com",
    apiKeyUrl: "https://aigocode.com/invite/CC-SWITCH",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.aigocode.com",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    baseURL: "https://api.aigocode.com",
    model: "gemini-3.1-pro",
    description: "AIGoCode",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aigocode",
    endpointCandidates: ["https://api.aigocode.com"],
    icon: "aigocode",
    iconColor: "#5B7FFF",
  },
  {
    name: "AICodeMirror",
    websiteUrl: "https://www.aicodemirror.com",
    apiKeyUrl: "https://www.aicodemirror.com/register?invitecode=9915W3",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.aicodemirror.com/api/gemini",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    baseURL: "https://api.aicodemirror.com/api/gemini",
    model: "gemini-3.1-pro",
    description: "AICodeMirror",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicodemirror",
    endpointCandidates: [
      "https://api.aicodemirror.com/api/gemini",
      "https://api.claudecode.net.cn/api/gemini",
    ],
    icon: "aicodemirror",
    iconColor: "#000000",
  },
  {
    name: "AICoding",
    websiteUrl: "https://aicoding.sh",
    apiKeyUrl: "https://aicoding.sh/i/CCSWITCH",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.aicoding.sh",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    baseURL: "https://api.aicoding.sh",
    model: "gemini-3.1-pro",
    description: "AICoding",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicoding",
    endpointCandidates: ["https://api.aicoding.sh"],
    icon: "aicoding",
    iconColor: "#000000",
  },
  {
    name: "CrazyRouter",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://cn.crazyrouter.com",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    baseURL: "https://cn.crazyrouter.com",
    model: "gemini-3.1-pro",
    description: "CrazyRouter",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "crazyrouter",
    endpointCandidates: ["https://cn.crazyrouter.com"],
    icon: "crazyrouter",
    iconColor: "#000000",
  },
  {
    name: "SSSAiCode",
    websiteUrl: "https://www.sssaicode.com",
    apiKeyUrl: "https://www.sssaicode.com/register?ref=DCP0SM",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://node-hk.sssaicode.com/api",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    baseURL: "https://node-hk.sssaicode.com/api",
    model: "gemini-3.1-pro",
    description: "SSSAiCode",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "sssaicode",
    endpointCandidates: [
      "https://node-hk.sssaicode.com/api",
      "https://claude2.sssaicode.com/api",
      "https://anti.sssaicode.com/api",
    ],
    icon: "sssaicode",
    iconColor: "#000000",
  },
  {
    name: "CTok.ai",
    websiteUrl: "https://ctok.ai",
    apiKeyUrl: "https://ctok.ai",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.ctok.ai/v1beta",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    baseURL: "https://api.ctok.ai/v1beta",
    model: "gemini-3.1-pro",
    description: "CTok",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "ctok",
    endpointCandidates: ["https://api.ctok.ai/v1beta"],
    icon: "ctok",
    iconColor: "#000000",
  },
  {
    name: "LionCCAPI",
    websiteUrl: "https://vibecodingapi.ai",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://vibecodingapi.ai",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    baseURL: "https://vibecodingapi.ai",
    model: "gemini-3.1-pro",
    description: "LionCCAPI",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "lionccapi",
    icon: "lioncc",
  },
  {
    name: "E-FlowCode",
    websiteUrl: "https://e-flowcode.cc",
    apiKeyUrl: "https://e-flowcode.cc",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://e-flowcode.cc",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.1-pro-preview",
      },
      config: {
        general: {
          previewFeatures: true,
          sessionRetention: {
            enabled: true,
            maxAge: "30d",
            warningAcknowledged: true,
          },
        },
        mcpServers: {},
        security: {
          auth: {
            selectedType: "gemini-api-key",
          },
        },
      },
    },
    baseURL: "https://e-flowcode.cc",
    model: "gemini-3.1-pro-preview",
    description: "E-FlowCode",
    category: "third_party",
    endpointCandidates: ["https://e-flowcode.cc"],
    icon: "eflowcode",
    iconColor: "#000000",
  },
  {
    name: "LemonData",
    websiteUrl: "https://lemondata.cc",
    apiKeyUrl: "https://lemondata.cc/r/FFX1ZDUP",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.lemondata.cc",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    baseURL: "https://api.lemondata.cc",
    model: "gemini-3.1-pro",
    description: "LemonData",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "lemondata",
    endpointCandidates: ["https://api.lemondata.cc"],
    icon: "lemondata",
  },
  {
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://openrouter.ai/api",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    baseURL: "https://openrouter.ai/api",
    model: "gemini-3.1-pro",
    description: "OpenRouter",
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
        GOOGLE_GEMINI_BASE_URL: "https://api.therouter.ai",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    baseURL: "https://api.therouter.ai",
    model: "gemini-3.1-pro",
    description: "TheRouter",
    category: "aggregator",
    endpointCandidates: ["https://api.therouter.ai"],
  },
  {
    name: "自定义",
    websiteUrl: "",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    model: "gemini-3.1-pro",
    description: "自定义 Gemini API 端点",
    category: "custom",
  },
];

export function getGeminiPresetByName(
  name: string,
): GeminiProviderPreset | undefined {
  return geminiProviderPresets.find((preset) => preset.name === name);
}

export function getGeminiPresetByUrl(
  url: string,
): GeminiProviderPreset | undefined {
  if (!url) return undefined;
  return geminiProviderPresets.find(
    (preset) =>
      preset.baseURL &&
      url.toLowerCase().includes(preset.baseURL.toLowerCase()),
  );
}
