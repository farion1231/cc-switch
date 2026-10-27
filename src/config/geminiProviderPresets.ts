import type { ProviderCategory } from "@/types";
import type { PresetFamilyFields } from "./presetFamilies";

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

export interface GeminiProviderPreset extends PresetFamilyFields {
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
  primePartner?: boolean; // 旧版的置顶合作伙伴标记；v7 起界面不再读取，新预设不写
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
  // ===== 赞助商预设：文件顺序与 README 赞助商表对齐（仅维护约定；应用内一律按显示名排序，不置顶）=====
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.ai",
    apiKeyUrl: "https://www.packyapi.ai/register?aff=cc-switch",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://www.packyapi.ai",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://www.packyapi.ai",
    model: "gemini-3.8-flash",
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
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://apinebula.ai",
    model: "gemini-3.8-flash",
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
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://api.aicodemirror.ai/api/gemini",
    model: "gemini-3.8-flash",
    description: "AICodeMirror",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicodemirror",
    endpointCandidates: ["https://api.aicodemirror.ai/api/gemini"],
    icon: "aicodemirror",
    iconColor: "#000000",
  },
  {
    name: "OpenModel",
    websiteUrl: "https://www.openmodel.ai",
    apiKeyUrl: "https://www.openmodel.ai?ref=cc-switch",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.openmodel.ai",
        GEMINI_MODEL: "gemini-4-argon",
      },
    },
    baseURL: "https://api.openmodel.ai",
    model: "gemini-4-argon",
    description: "OpenModel",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "openmodel",
    icon: "openmodel",
  },
  {
    name: "DIT.ai",
    websiteUrl: "https://dit.ai",
    apiKeyUrl: "https://dit.ai?utm_source=ccswitch",
    // Gemini 原生 /v1beta 在站点根；网关也认 x-goog-api-key（2026-10-21 用假 Key
    // 探测为 invalid_api_key 而非 missing_api_key）
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.dit.ai",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://api.dit.ai",
    model: "gemini-3.8-flash",
    description: "DIT.ai",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "dit",
    icon: "dit",
  },
  {
    name: "V-API",
    websiteUrl: "https://gpt.ge",
    apiKeyUrl: "https://gpt.ge/signup?aff=URNP",
    // Gemini 原生 /v1beta（含 streamGenerateContent）在站点根，令牌选 gemini 或
    // gemini_cli 分组（gpt.ge/models/gemini-3.8-flash，2026-10-23）
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.gpt.ge",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://api.gpt.ge",
    model: "gemini-3.8-flash",
    description: "V-API",
    category: "aggregator",
    // 备用线路取自 gpt.ge/api/status 的 api_list（2026-10-23）
    endpointCandidates: [
      "https://api.gpt.ge",
      "https://api.vveai.com",
      "https://run.v3.cm",
      "https://api1.v3.cm",
      "https://api2.v3.cm",
      "https://us.gpt.ge",
    ],
    isPartner: true,
    partnerPromotionKey: "vapi",
    icon: "vapi",
  },
  {
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://router.shengsuanyun.com/api",
        GEMINI_MODEL: "google/gemini-3.8-flash",
      },
    },
    baseURL: "https://router.shengsuanyun.com/api",
    model: "google/gemini-3.8-flash",
    description: "Shengsuanyun",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "shengsuanyun",
    icon: "shengsuanyun",
  },
  {
    name: "AIGoCode",
    websiteUrl: "https://aigocode.net",
    apiKeyUrl: "https://aigocode.net/invite/CC-SWITCH",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.aigocode.dev",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://api.aigocode.dev",
    model: "gemini-3.8-flash",
    description: "AIGoCode",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aigocode",
    endpointCandidates: [
      "https://api.aigocode.dev",
      "https://api.aigocode.app",
    ],
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
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://api.qnaigc.com/bypass/vertex",
    model: "gemini-3.8-flash",
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
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://api.aicoding.inc",
    model: "gemini-3.8-flash",
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
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://subrouter.ai/v1beta",
    model: "gemini-3.8-flash",
    description: "SubRouter",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "subrouter",
    endpointCandidates: ["https://subrouter.ai/v1beta"],
    icon: "subrouter",
  },
  {
    name: "88API",
    websiteUrl: "https://88api.ai",
    apiKeyUrl: "https://88api.ai/sign-up?aff=HSGY",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.88api.ai",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://api.88api.ai",
    model: "gemini-3.8-flash",
    description: "88API",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "88api",
    endpointCandidates: ["https://api.88api.ai", "https://88api.ai"],
    icon: "88api",
  },
  {
    name: "APIKEY.FUN",
    websiteUrl: "https://apikey.fan",
    apiKeyUrl: "https://apikey.fan/register?aff=CCSwitch",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.apikey.fan",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://api.apikey.fan",
    model: "gemini-3.8-flash",
    description: "APIKEY.FUN",
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apikeyfun",
    endpointCandidates: [
      "https://api.apikey.fan",
      "https://api.apikey.fun",
      "https://slb.apikey.fun",
    ],
    icon: "apikeyfun",
  },
  {
    name: "9527CODE",
    websiteUrl: "https://9527.codes",
    apiKeyUrl: "https://9527.codes/register?aff=e5zI",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://9527.codes",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://9527.codes",
    model: "gemini-3.8-flash",
    description: "9527CODE",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "9527code",
    endpointCandidates: [
      "https://9527.codes",
      "https://api.9527.codes",
      "https://cdn.9527.codes",
    ],
    icon: "9527code",
  },
  {
    name: "Code0",
    websiteUrl: "https://code0.ai",
    apiKeyUrl: "https://code0.ai/agent/register/B2XHxGjGmRvqgznY",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://code0.ai",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://code0.ai",
    model: "gemini-3.8-flash",
    description: "Code0",
    category: "aggregator",
    icon: "code0",
  },
  {
    name: "A6API",
    websiteUrl: "https://www.a6api.com",
    apiKeyUrl: "https://a6api.com/register?aff=AqNr",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.a6api.com",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://api.a6api.com",
    model: "gemini-3.8-flash",
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
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://node-hk.sssaicodeapi.com/api",
    model: "gemini-3.8-flash",
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
    name: "SoleAPI",
    websiteUrl: "https://soleapi.com",
    apiKeyUrl: "https://soleapi.com/r/ccswitch",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://soleapi.com",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://soleapi.com",
    model: "gemini-3.8-flash",
    description: "SoleAPI",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "soleapi",
    endpointCandidates: ["https://soleapi.com"],
    icon: "soleapi",
  },
  {
    name: "Cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.cubence.com",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://api.cubence.com",
    model: "gemini-3.8-flash",
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
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://cn.crazyrouter.com",
    model: "gemini-3.8-flash",
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
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://sudocode.us",
    model: "gemini-3.8-flash",
    description: "SudoCode.us",
    category: "third_party",
    isPartner: true,
    endpointCandidates: ["https://sudocode.us", "https://sudocode.run"],
    icon: "sudocode-us",
  },
  {
    name: "XycAi",
    websiteUrl: "https://xycai.us",
    apiKeyUrl: "https://xycai.us/register?aff=Uhu9",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://apicdn.xycai.us",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://apicdn.xycai.us",
    model: "gemini-3.8-flash",
    description: "XycAi",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "xycai",
    endpointCandidates: ["https://apicdn.xycai.us", "https://apicdn.xyc.ai"],
    icon: "xycai",
  },
  {
    name: "Modelflare",
    websiteUrl: "https://modelflare.dev",
    apiKeyUrl:
      "https://modelflare.dev/sign-up?utm_medium=referral&utm_source=ccswitch",
    // 官方 Gemini CLI 文档（docs.modelflare.dev/guides/clients/gemini-cli，
    // 2026-10-15 核对）：base 用站点根。Gemini CLI 走 /v1beta，只有
    // modelflare.dev 提供，origin 子域不提供，故不加备用端点
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://modelflare.dev",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://modelflare.dev",
    model: "gemini-3.8-flash",
    description: "Modelflare",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "modelflare",
    icon: "modelflare",
  },
  {
    name: "AllTokens",
    websiteUrl: "https://alltokens.cc",
    apiKeyUrl: "https://alltokens.cc/register",
    // Sub2API 网关：Key 绑定单个分组，须用 Gemini 分组的 Key（分组与模型见
    // alltokens.cc/api/v1/model-plaza，2026-10-17 核对）
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://alltokens.cc",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://alltokens.cc",
    model: "gemini-3.8-flash",
    description: "AllTokens",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "alltokens",
    icon: "alltokens",
  },
  {
    name: "AllTokens SI",
    websiteUrl: "https://alltokens.si",
    // 站点关闭了个人注册（/api/status 的 register_enabled=false），只能企业注册
    apiKeyUrl: "https://alltokens.si/enterprise-register",
    // Gemini 分组不在默认自动分组里，须用 Gemini 分组的 Key；目录里唯一
    // 规范的 3.8 Flash ID 是 gemini-3.8-flash-high（alltokens.si/api/pricing，
    // 2026-10-19 核对）
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://alltokens.si",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.8-flash-high",
      },
    },
    baseURL: "https://alltokens.si",
    model: "gemini-3.8-flash-high",
    description: "AllTokens SI",
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "alltokens_si",
    icon: "alltokens-si",
  },
  // ===== 非赞助商预设：应用内展示按显示名排序，此处文件顺序不影响展示 =====
  {
    name: "Tu-zi",
    nameKey: "providerForm.presets.tuzi",
    websiteUrl: "https://api.tu-zi.com",
    apiKeyUrl: "https://api.tu-zi.com/token",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.tu-zi.com",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://api.tu-zi.com",
    model: "gemini-3.8-flash",
    description: "Tu-zi",
    category: "aggregator",
    endpointCandidates: [
      "https://api.tu-zi.com",
      "https://api.ourzhishi.top",
      "https://api.sydney-ai.com",
      "https://apicdn.tu-zi.com",
    ],
    icon: "tuzi",
  },
  {
    name: "E-FlowCode",
    websiteUrl: "https://e-flowcode.cc",
    apiKeyUrl: "https://e-flowcode.cc",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://e-flowcode.cc",
        GEMINI_API_KEY: "",
        GEMINI_MODEL: "gemini-3.8-flash",
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
    model: "gemini-3.8-flash",
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
        GEMINI_MODEL: "google/gemini-3.8-flash",
      },
    },
    baseURL: "https://open.cherryin.net",
    model: "google/gemini-3.8-flash",
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
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://openrouter.ai/api",
    model: "gemini-3.8-flash",
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
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://api.therouter.ai",
    model: "gemini-3.8-flash",
    description: "TheRouter",
    category: "aggregator",
    endpointCandidates: ["https://api.therouter.ai"],
    icon: "therouter",
  },
  {
    name: "AICodeWith",
    websiteUrl: "https://aicodewith.ai",
    apiKeyUrl: "https://aicodewith.ai/login?tab=register",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.aicodewith.ai/gemini_cli",
        GEMINI_MODEL: "gemini-4-argon",
      },
    },
    baseURL: "https://api.aicodewith.ai/gemini_cli",
    model: "gemini-4-argon",
    description: "AICodeWith",
    category: "aggregator",
    endpointCandidates: ["https://api.aicodewith.ai/gemini_cli"],
    icon: "aicodewith",
    iconColor: "#3A3B40",
  },
  {
    name: "SoruxGPT",
    websiteUrl: "https://ai.soruxgpt.com",
    apiKeyUrl: "https://ai.soruxgpt.com/console/token",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://ai.soruxgpt.com",
        GEMINI_MODEL: "gemini-4-argon",
      },
    },
    baseURL: "https://ai.soruxgpt.com",
    model: "gemini-4-argon",
    description: "SoruxGPT",
    category: "aggregator",
    endpointCandidates: [
      "https://ai.soruxgpt.com",
      "https://api-cf.soruxgpt.com",
      "https://api-host.soruxgpt.com",
      "https://api-node6.soruxgpt.com",
      "https://api-node3.soruxgpt.com",
    ],
    icon: "soruxgpt",
  },
  {
    name: "IKunCode",
    websiteUrl: "https://api.ikuncode.ai",
    apiKeyUrl: "https://api.ikuncode.ai/console/token",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.ikuncode.ai",
        GEMINI_MODEL: "gemini-4-argon",
      },
    },
    baseURL: "https://api.ikuncode.ai",
    model: "gemini-4-argon",
    description: "IKunCode",
    category: "aggregator",
    endpointCandidates: ["https://api.ikuncode.ai", "https://api.ikuncode.cc"],
    icon: "ikuncode",
  },
  {
    name: "RunToken",
    websiteUrl: "https://www.runtoken.ai",
    apiKeyUrl: "https://www.runtoken.ai/console/token",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://www.runtoken.ai",
        GEMINI_MODEL: "gemini-4-argon",
      },
    },
    baseURL: "https://www.runtoken.ai",
    model: "gemini-4-argon",
    description: "RunToken",
    category: "aggregator",
    endpointCandidates: ["https://www.runtoken.ai", "https://fast.runtoken.ai"],
    icon: "runtoken",
  },
  {
    name: "YYLX",
    nameKey: "providerForm.presets.yylx",
    websiteUrl: "https://yylx.io",
    apiKeyUrl: "https://app.yylx.io/keys",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://app.yylx.io",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://app.yylx.io",
    model: "gemini-3.8-flash",
    description: "YYLX",
    category: "aggregator",
    endpointCandidates: [
      "https://app.yylx.io",
      "https://global.yylx.io",
      "https://cn.yylx.io",
    ],
    icon: "yylx",
  },
  {
    name: "TokenFlux",
    websiteUrl: "https://tokenflux.dev",
    apiKeyUrl: "https://tokenflux.dev/keys",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://tokenflux.dev",
        GEMINI_MODEL: "gemini-3.8-flash",
      },
    },
    baseURL: "https://tokenflux.dev",
    model: "gemini-3.8-flash",
    description: "TokenFlux",
    category: "aggregator",
    icon: "tokenflux",
  },
  {
    name: "AIComing",
    websiteUrl: "https://aicoming.top",
    apiKeyUrl: "https://aicoming.top/console#keys",
    settingsConfig: {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://api.aicoming.top",
        GEMINI_MODEL: "gemini-4-argon",
      },
    },
    baseURL: "https://api.aicoming.top",
    model: "gemini-4-argon",
    description: "AIComing",
    category: "aggregator",
    icon: "aicoming",
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
