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
  primePartner?: boolean; // 置顶合作伙伴（顶级）：徽章显示为心形
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
    websiteUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://router.shengsuanyun.com/api",
        GEMINI_MODEL: "google/gemini-3.5-flash",
      },
    },
    baseURL: "https://router.shengsuanyun.com/api",
    model: "google/gemini-3.5-flash",
    description: "Shengsuanyun",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "shengsuanyun",
    icon: "shengsuanyun",
  },
  {
    name: "Unity2.ai",
    websiteUrl: "https://unity2.ai",
    apiKeyUrl: "https://unity2.ai/register?source=ccs",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.unity2.ai",
        GEMINI_MODEL: "gemini-3.1-pro",
      },
    },
    baseURL: "https://api.unity2.ai",
    model: "gemini-3.1-pro",
    description: "Unity2.ai",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "unity2",
    icon: "unity2",
  },
  {
    name: "SubRouter",
    websiteUrl: "https://subrouter.ai",
    apiKeyUrl: "https://subrouter.ai/register?aff=l3ri",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://subrouter.ai/v1beta",
        GEMINI_MODEL: "gemini-3.5-flash",
      },
    },
    baseURL: "https://subrouter.ai/v1beta",
    model: "gemini-3.5-flash",
    description: "SubRouter",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "subrouter",
    endpointCandidates: ["https://subrouter.ai/v1beta"],
    icon: "subrouter",
  },
  {
    name: "Qiniu",
    nameKey: "providerForm.presets.qiniu",
    websiteUrl: "https://s.qiniu.com/nMvAvy",
    apiKeyUrl: "https://s.qiniu.com/nMvAvy",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.qnaigc.com/bypass/vertex",
        GEMINI_MODEL: "gemini-3.1-pro-preview",
      },
    },
    baseURL: "https://api.qnaigc.com/bypass/vertex",
    model: "gemini-3.1-pro-preview",
    description: "Qiniu",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "qiniu",
    endpointCandidates: [
      "https://api.qnaigc.com/bypass/vertex",
      "https://api.modelink.ai/bypass/vertex",
    ],
    icon: "qiniu",
  },
  {
    name: "Code0",
    websiteUrl: "https://code0.ai",
    apiKeyUrl: "https://code0.ai/agent/register/B2XHxGjGmRvqgznY",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://code0.ai",
        GEMINI_MODEL: "gemini-3.1-pro-preview",
      },
    },
    baseURL: "https://code0.ai",
    model: "gemini-3.1-pro-preview",
    description: "Code0",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "code0",
    icon: "code0",
  },
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.com",
    apiKeyUrl: "https://www.packyapi.com/register?aff=cc-switch",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://www.packyapi.com",
        GEMINI_MODEL: "gemini-3.5-flash",
      },
    },
    baseURL: "https://www.packyapi.com",
    model: "gemini-3.5-flash",
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
    name: "APIKEY.FUN",
    websiteUrl: "https://apikey.fun",
    apiKeyUrl: "https://apikey.fun/register?aff=CCSwitch",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.apikey.fun",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.5-flash",
      },
    },
    baseURL: "https://api.apikey.fun",
    model: "gemini-3.5-flash",
    description: "APIKEY.FUN",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apikeyfun",
    endpointCandidates: ["https://api.apikey.fun", "https://slb.apikey.fun"],
    icon: "apikeyfun",
  },
  {
    name: "APINebula",
    websiteUrl: "https://apinebula.com",
    apiKeyUrl: "https://apinebula.com/02rw5X",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://apinebula.com",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.5-flash",
      },
    },
    baseURL: "https://apinebula.com",
    model: "gemini-3.5-flash",
    description: "APINebula",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apinebula",
    endpointCandidates: ["https://apinebula.com"],
    icon: "apinebula",
  },
  {
    name: "Cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.cubence.com",
        GEMINI_MODEL: "gemini-3.5-flash",
      },
    },
    baseURL: "https://api.cubence.com",
    model: "gemini-3.5-flash",
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
        GEMINI_MODEL: "gemini-3.5-flash",
      },
    },
    baseURL: "https://api.aigocode.com",
    model: "gemini-3.5-flash",
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
        GEMINI_MODEL: "gemini-3.5-flash",
      },
    },
    baseURL: "https://api.aicodemirror.com/api/gemini",
    model: "gemini-3.5-flash",
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
    name: "CrazyRouter",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://cn.crazyrouter.com",
        GEMINI_MODEL: "gemini-3.5-flash",
      },
    },
    baseURL: "https://cn.crazyrouter.com",
    model: "gemini-3.5-flash",
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
    websiteUrl: "https://sssaicodeapi.com",
    apiKeyUrl: "https://sssaicodeapi.com/register?ref=DCP0SM",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://node-hk.sssaicodeapi.com/api",
        GEMINI_MODEL: "gemini-3.5-flash",
      },
    },
    baseURL: "https://node-hk.sssaicodeapi.com/api",
    model: "gemini-3.5-flash",
    description: "SSSAiCode",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "sssaicode",
    endpointCandidates: [
      "https://node-hk.sssaicodeapi.com/api",
      "https://node-hk.sssaiapi.com/api",
      "https://node-cf.sssaicodeapi.com/api",
    ],
    icon: "sssaicode",
    iconColor: "#000000",
  },
  {
    name: "ETok.ai",
    websiteUrl: "https://etok.ai",
    apiKeyUrl: "https://etok.ai",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.etok.ai/v1beta",
        GEMINI_MODEL: "gemini-3.5-flash",
      },
    },
    baseURL: "https://api.etok.ai/v1beta",
    model: "gemini-3.5-flash",
    description: "ETok",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "etok",
    endpointCandidates: ["https://api.etok.ai/v1beta"],
    icon: "etok",
    iconColor: "#000000",
  },
  {
    name: "E-FlowCode",
    websiteUrl: "https://e-flowcode.cc",
    apiKeyUrl: "https://e-flowcode.cc",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://e-flowcode.cc",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.5-flash",
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
    model: "gemini-3.5-flash",
    description: "E-FlowCode",
    category: "third_party",
    endpointCandidates: ["https://e-flowcode.cc"],
    icon: "eflowcode",
    iconColor: "#000000",
  },
  {
    name: "CherryIN",
    websiteUrl: "https://open.cherryin.ai",
    apiKeyUrl: "https://open.cherryin.ai/console/token",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://open.cherryin.net",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "google/gemini-3.5-flash",
      },
    },
    baseURL: "https://open.cherryin.net",
    model: "google/gemini-3.5-flash",
    description: "CherryIN",
    category: "aggregator",
    endpointCandidates: ["https://open.cherryin.net"],
    icon: "cherryin",
  },
  {
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://openrouter.ai/api",
        GEMINI_MODEL: "gemini-3.5-flash",
      },
    },
    baseURL: "https://openrouter.ai/api",
    model: "gemini-3.5-flash",
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
        GEMINI_MODEL: "gemini-3.5-flash",
      },
    },
    baseURL: "https://api.therouter.ai",
    model: "gemini-3.5-flash",
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
        GEMINI_MODEL: "gemini-3.5-flash",
      },
    },
    model: "gemini-3.5-flash",
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
