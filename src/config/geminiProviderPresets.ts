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
  // ===== 赞助商预设：文件顺序 = 应用内展示顺序，与 README 赞助商表对齐 =====
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.ai",
    apiKeyUrl: "https://www.packyapi.ai/register?aff=cc-switch",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://www.packyapi.ai",
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://www.packyapi.ai",
    model: "gemini-3.6-flash",
    description: "PackyCode",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "packycode",
    endpointCandidates: [
      "https://www.packyapi.ai",
      "https://cf.api.fan",
      "https://slb-v1.api.fan",
      "https://www.packyapi.com",
    ],
    icon: "packycode",
  },
  {
    name: "APINebula",
    websiteUrl: "https://apinebula.ai",
    apiKeyUrl: "https://apinebula.ai/VjM74M",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://apinebula.ai",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://apinebula.ai",
    model: "gemini-3.6-flash",
    description: "APINebula",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apinebula",
    endpointCandidates: ["https://apinebula.ai"],
    icon: "apinebula",
  },
  {
    name: "AICodeMirror",
    websiteUrl: "https://www.aicodemirror.ai",
    apiKeyUrl: "https://www.aicodemirror.ai/register?invitecode=9915W3",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.aicodemirror.ai/api/gemini",
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://api.aicodemirror.ai/api/gemini",
    model: "gemini-3.6-flash",
    description: "AICodeMirror",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicodemirror",
    endpointCandidates: ["https://api.aicodemirror.ai/api/gemini"],
    icon: "aicodemirror",
    iconColor: "#000000",
  },
  {
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://router.shengsuanyun.com/api",
        GEMINI_MODEL: "google/gemini-3.6-flash",
      },
    },
    baseURL: "https://router.shengsuanyun.com/api",
    model: "google/gemini-3.6-flash",
    description: "Shengsuanyun",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "shengsuanyun",
    icon: "shengsuanyun",
  },
  {
    name: "AIGoCode",
    websiteUrl: "https://aigocode.app",
    apiKeyUrl: "https://aigocode.app/invite/CC-SWITCH",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.aigocode.app",
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://api.aigocode.app",
    model: "gemini-3.6-flash",
    description: "AIGoCode",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aigocode",
    endpointCandidates: ["https://api.aigocode.app"],
    icon: "aigocode",
    iconColor: "#5B7FFF",
  },
  {
    name: "Qiniu",
    nameKey: "providerForm.presets.qiniu",
    websiteUrl: "https://s.qiniu.com/nMvAvy",
    apiKeyUrl: "https://s.qiniu.com/nMvAvy",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.qnaigc.com/bypass/vertex",
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://api.qnaigc.com/bypass/vertex",
    model: "gemini-3.6-flash",
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
    name: "AICoding",
    websiteUrl: "https://aicoding.inc",
    apiKeyUrl: "https://aicoding.inc/i/CCSWITCH",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.aicoding.inc",
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://api.aicoding.inc",
    model: "gemini-3.6-flash",
    description: "AICoding",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicoding",
    endpointCandidates: ["https://api.aicoding.inc"],
    icon: "aicoding",
    iconColor: "#000000",
  },
  {
    name: "SubRouter",
    websiteUrl: "https://subrouter.ai",
    apiKeyUrl: "https://subrouter.ai/register?aff=l3ri",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://subrouter.ai/v1beta",
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://subrouter.ai/v1beta",
    model: "gemini-3.6-flash",
    description: "SubRouter",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "subrouter",
    endpointCandidates: ["https://subrouter.ai/v1beta"],
    icon: "subrouter",
  },
  {
    name: "APIKEY.FUN",
    websiteUrl: "https://apikey.fun",
    apiKeyUrl: "https://apikey.fun/register?aff=CCSwitch",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.apikey.fun",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://api.apikey.fun",
    model: "gemini-3.6-flash",
    description: "APIKEY.FUN",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apikeyfun",
    endpointCandidates: ["https://api.apikey.fun", "https://slb.apikey.fun"],
    icon: "apikeyfun",
  },
  {
    name: "Code0",
    websiteUrl: "https://code0.ai",
    apiKeyUrl: "https://code0.ai/agent/register/B2XHxGjGmRvqgznY",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://code0.ai",
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://code0.ai",
    model: "gemini-3.6-flash",
    description: "Code0",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "code0",
    icon: "code0",
  },
  {
    name: "A6API",
    websiteUrl: "https://www.a6api.com",
    apiKeyUrl: "https://a6api.com/register?aff=AqNr",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.a6api.com",
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://api.a6api.com",
    model: "gemini-3.6-flash",
    description: "A6API",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "a6api",
    icon: "a6api",
  },
  {
    name: "SSSAiCode",
    websiteUrl: "https://sssaicodeapi.com",
    apiKeyUrl: "https://sssaicodeapi.com/register?ref=DCP0SM",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://node-hk.sssaicodeapi.com/api",
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://node-hk.sssaicodeapi.com/api",
    model: "gemini-3.6-flash",
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
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://api.etok.ai/v1beta",
    model: "gemini-3.6-flash",
    description: "ETok",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "etok",
    endpointCandidates: ["https://api.etok.ai/v1beta"],
    icon: "etok",
    iconColor: "#000000",
  },
  {
    name: "Cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.cubence.com",
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://api.cubence.com",
    model: "gemini-3.6-flash",
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
    name: "CrazyRouter",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://cn.crazyrouter.com",
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://cn.crazyrouter.com",
    model: "gemini-3.6-flash",
    description: "CrazyRouter",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "crazyrouter",
    endpointCandidates: ["https://cn.crazyrouter.com"],
    icon: "crazyrouter",
    iconColor: "#000000",
  },
  {
    name: "SudoCode.us",
    websiteUrl: "https://sudocode.us",
    apiKeyUrl: "https://sudocode.us",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://sudocode.us",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.1-flash-lite",
      },
    },
    baseURL: "https://sudocode.us",
    model: "gemini-3.1-flash-lite",
    description: "SudoCode.us",
    category: "third_party",
    isPartner: true,
    endpointCandidates: ["https://sudocode.us", "https://sudocode.run"],
    icon: "sudocode-us",
  },
  // ===== 非赞助商预设：应用内展示按显示名排序，此处文件顺序不影响展示 =====
  {
    name: "E-FlowCode",
    websiteUrl: "https://e-flowcode.cc",
    apiKeyUrl: "https://e-flowcode.cc",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://e-flowcode.cc",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.6-flash",
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
    model: "gemini-3.6-flash",
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
        GEMINI_MODEL: "google/gemini-3.6-flash",
      },
    },
    baseURL: "https://open.cherryin.net",
    model: "google/gemini-3.6-flash",
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
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://openrouter.ai/api",
    model: "gemini-3.6-flash",
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
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    baseURL: "https://api.therouter.ai",
    model: "gemini-3.6-flash",
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
        GEMINI_MODEL: "gemini-3.6-flash",
      },
    },
    model: "gemini-3.6-flash",
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
