/**
 * 预设供应商配置模板
 */
import { ProviderCategory } from "../types";
import type { PresetFamilyFields } from "./presetFamilies";

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

export interface ProviderPreset extends PresetFamilyFields {
  name: string;
  nameKey?: string; // i18n key for localized display name
  websiteUrl: string;
  // 新增：第三方/聚合等可单独配置获取 API Key 的链接
  apiKeyUrl?: string;
  settingsConfig: object;
  isOfficial?: boolean; // 标识是否为官方预设
  isPartner?: boolean; // 标识是否为商业合作伙伴
  primePartner?: boolean; // 旧版的置顶合作伙伴标记；v7 起界面不再读取，新预设不写
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
  // - "gemini_native": Gemini Native generateContent API 格式，需要格式转换
  apiFormat?:
    | "anthropic"
    | "openai_chat"
    | "openai_responses"
    | "gemini_native";

  // 供应商类型标识（用于特殊供应商检测）
  // - "github_copilot": GitHub Copilot 供应商（需要 OAuth 认证）
  // - "codex_oauth": OpenAI Codex via ChatGPT Plus/Pro 反代（需要 OAuth 认证）
  providerType?: "github_copilot" | "codex_oauth" | "xai_oauth";

  // 是否需要 OAuth 认证（而非 API Key）
  requiresOAuth?: boolean;

  // 是否在 UI 中隐藏该预设（预设仍存在，仅不在列表中显示）
  hidden?: boolean;

  // 获取模型列表使用的完整 URL（覆写自动候选逻辑）
  // 缺省时后端基于 baseURL 自动尝试 /v1/models、/models 以及剥离已知兼容子路径后的变体。
  modelsUrl?: string;
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
  // ===== 赞助商预设：文件顺序与 README 赞助商表对齐（仅维护约定；应用内一律按显示名排序，不置顶）=====
  {
    name: "Kimi",
    family: "kimi",
    planKey: "payg",
    regionKey: "cn",
    primePartner: true,
    websiteUrl:
      "https://platform.kimi.com?track_id=track-7cf2b91dcde043eda6ef9a95951a042c&aff=cc-switch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.moonshot.cn/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "kimi-k2.7-code",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "kimi-k2.7-code",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "kimi-k2.7-code",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "kimi-k2.7-code",
      },
    },
    category: "cn_official",
    partnerPromotionKey: "kimi",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  // API 开放平台海外/Global 变体：platform.kimi.ai + api.moonshot.ai 端点
  {
    name: "Kimi Global",
    family: "kimi",
    planKey: "payg",
    regionKey: "intl",
    websiteUrl:
      "https://platform.kimi.ai?track_id=track-674ed6e2af924a5682a87421f7cf753a&aff=cc-switch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.moonshot.ai/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "kimi-k2.7-code",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "kimi-k2.7-code",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "kimi-k2.7-code",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "kimi-k2.7-code",
      },
    },
    category: "cn_official",
    partnerPromotionKey: "kimi",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "Kimi For Coding",
    family: "kimi",
    planKey: "coding",
    regionKey: "cn",
    primePartner: true,
    websiteUrl: "https://www.kimi.com/code/?aff=cc-switch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.kimi.com/coding/",
        ANTHROPIC_AUTH_TOKEN: "",
        // CLAUDE_CODE_MAX_CONTEXT_TOKENS 只对非 claude- 前缀模型 id 生效，
        // 必须显式路由端点别名 kimi-for-coding（与 codex/hermes/opencode 预设一致）
        ANTHROPIC_MODEL: "kimi-for-coding",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "kimi-for-coding",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "kimi-for-coding",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "kimi-for-coding",
        // 双键钉 256K：压缩窗口=min(模型窗口,值)，与窗口同值时行为等价于不设，
        // 但显式钉住可屏蔽远程实验下发的更小压缩点；调整直接改 JSON，不出表单字段
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "262144",
        CLAUDE_CODE_AUTO_COMPACT_WINDOW: "262144",
      },
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "Kimi For Coding Global",
    family: "kimi",
    planKey: "coding",
    regionKey: "intl",
    websiteUrl: "https://www.kimi.ai/code?aff=cc-switch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.kimi.ai/coding/",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "kimi-for-coding",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "kimi-for-coding",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "kimi-for-coding",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "kimi-for-coding",
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "262144",
        CLAUDE_CODE_AUTO_COMPACT_WINDOW: "262144",
      },
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.ai",
    apiKeyUrl: "https://www.packyapi.ai/register?aff=cc-switch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://www.packyapi.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    // 请求地址候选（用于地址管理/测速）
    endpointCandidates: [
      "https://www.packyapi.ai",
      "https://cf.api.fan",
      "https://slb-v1.api.fan",
      "https://www.packyapi.com",
    ],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "packycode", // 促销信息 i18n key
    icon: "packycode",
  },
  {
    name: "APINebula",
    websiteUrl: "https://apinebula.ai",
    apiKeyUrl: "https://apinebula.ai/VjM74M",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://apinebula.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://apinebula.ai"],
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apinebula",
    icon: "apinebula",
  },
  {
    name: "AICodeMirror",
    websiteUrl: "https://www.aicodemirror.ai",
    apiKeyUrl: "https://www.aicodemirror.ai/register?invitecode=9915W3",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.aicodemirror.ai/api/claudecode",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://api.aicodemirror.ai/api/claudecode"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "aicodemirror", // 促销信息 i18n key
    icon: "aicodemirror",
    iconColor: "#000000",
  },
  {
    name: "PatewayAI",
    websiteUrl: "https://pateway.ai",
    apiKeyUrl: "https://pateway.ai/?ch=etzpm8&aff=WB6M6F67#/",
    apiKeyField: "ANTHROPIC_API_KEY",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.pateway.ai",
        ANTHROPIC_API_KEY: "",
      },
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "patewayai",
    icon: "pateway",
  },
  {
    name: "hao.ai",
    websiteUrl: "https://hao.ai",
    apiKeyUrl: "https://hao.ai/invite/DTBZS3OR",
    // 接入文档 hao.ai/docs/integrations/claude-code/model-provider 与
    // hao.ai/vibe-coding（2026-10-21 核对）：Anthropic Messages 在 /anthropic，
    // 只配地址和 Key、模型留空即用 Claude Code 默认模型。目录里的 ID 带 anthropic/
    // 前缀，但官方给 Claude Code 的配置不覆盖模型
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.hao.ai/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "haoai",
    icon: "haoai",
  },
  {
    name: "FennoAI",
    websiteUrl: "https://api.fenno.ai",
    apiKeyUrl:
      "https://api.fenno.ai/register?redirect=/purchase?tab=subscription%26group=16&aff=P9MR3D3PLCNL",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.fenno.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "fenno",
    icon: "fenno",
  },
  {
    name: "RunAPI",
    websiteUrl: "https://runapi.host",
    apiKeyUrl: "https://runapi.host/register?aff=iOKB",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://runapi.host",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://runapi.host", "https://runapi.co"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "runapi",
    icon: "runapi",
  },
  {
    name: "OpenModel",
    websiteUrl: "https://www.openmodel.ai",
    apiKeyUrl: "https://www.openmodel.ai?ref=cc-switch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.openmodel.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "openmodel",
    icon: "openmodel",
  },
  {
    name: "EveryAPI",
    websiteUrl: "https://everyapi.ai",
    apiKeyUrl:
      "https://app.everyapi.ai/signin?aff=FqlMa2eVVY&utm_source=ccswitch&utm_medium=invite",
    // 官网接入说明（everyapi.ai，2026-10-17 核对）：Anthropic 客户端 base 用
    // 站点根（不带 /v1）、Key 填 AUTH_TOKEN。各 Claude 分组都有 haiku-5-5，
    // 四档跟随 Claude Code 默认即可。api-cn 是官方的国内优先线路
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.everyapi.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: [
      "https://api.everyapi.ai",
      "https://api-cn.everyapi.ai",
    ],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "everyapi",
    icon: "everyapi",
  },
  {
    name: "GrooRoute Pro",
    websiteUrl: "https://grooroute.pro",
    apiKeyUrl: "https://grooroute.pro/sign-up?aff=qqm9",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://grooroute.pro",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-sonnet-5-5",
      },
    },
    endpointCandidates: ["https://grooroute.pro", "https://grooroute.xyz"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "grooroute",
    icon: "grooroute",
  },
  {
    name: "DIT.ai",
    websiteUrl: "https://dit.ai",
    apiKeyUrl: "https://dit.ai/register",
    // 协议文档 dit.ai/docs、模型目录 dit.ai/models（2026-10-21 核对）：Anthropic
    // Messages 在站点根、Key 走 Bearer。Claude 系有 sonnet-5 / opus-5 / sonnet-5-5 /
    // opus-5-5，Haiku 只有带日期的 claude-haiku-4-5-20251001，没有 haiku-5-5
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.dit.ai",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-haiku-4-5-20251001",
      },
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "dit",
    icon: "dit",
  },
  {
    name: "TokenFactory",
    nameKey: "providerForm.presets.tokenfactory",
    websiteUrl: "https://tokenfactory.cn",
    apiKeyUrl: "https://console.tokenfactory.cn/login",
    // 官方 Claude Code 文档（docs.tokenfactory.cn/tools/claude-code，
    // 2026-10-15 核对）：base 保留 /anthropic、Key 填 AUTH_TOKEN。平台只有
    // 国产模型；Haiku 档（后台任务与 Explore 子代理）用 deepseek-v4.1-flash，
    // 它在 Anthropic 接口下的工具调用已核验，glm-5.3-flash 平台不承诺。
    // 模型窗口均 1M（api.tokenfactory.cn/v1/public/models），全档预勾 1M。带
    // context-1m beta 头的请求能过网关的请求头校验（假 Key 实测照常走到
    // 鉴权 401），未经真 Key 实测
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.tokenfactory.cn/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "glm-5.3[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "deepseek-v4.1-flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.3[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "kimi-k3[1M]",
      },
    },
    endpointCandidates: ["https://api.tokenfactory.cn/anthropic"],
    category: "aggregator",
    icon: "tokenfactory",
  },
  {
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://router.shengsuanyun.com/api",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "anthropic/claude-sonnet-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "anthropic/claude-haiku-4.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "anthropic/claude-sonnet-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "anthropic/claude-opus-5",
      },
    },
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
        ANTHROPIC_BASE_URL: "https://api.aigocode.dev",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    // 请求地址候选（用于地址管理/测速）
    endpointCandidates: [
      "https://api.aigocode.dev",
      "https://api.aigocode.app",
    ],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "aigocode", // 促销信息 i18n key
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
        ANTHROPIC_BASE_URL: "https://api.qnaigc.com",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://api.qnaigc.com", "https://api.modelink.ai"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "qiniu",
    icon: "qiniu",
  },
  {
    name: "AICoding",
    websiteUrl: "https://aicoding.inc",
    apiKeyUrl: "https://aicoding.inc/i/CCSWITCH",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.aicoding.inc",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://api.aicoding.inc"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "aicoding", // 促销信息 i18n key
    icon: "aicoding",
    iconColor: "#000000",
  },
  {
    name: "SubRouter",
    websiteUrl: "https://subrouter.ai",
    apiKeyUrl: "https://subrouter.ai/register?aff=l3ri",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://subrouter.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "subrouter",
    icon: "subrouter",
  },
  {
    // FluxA AgentMarket 以合作价转售的百度智能云 TokenPlan：产品页写明
    // "purchase it through AgentMarket, then use Baidu AI Cloud's endpoint and
    // API key directly"，端点取其所链的百度国际站 Token Plan Enterprise 文档
    // （2026-09-16 版）team 专属基址 —— 与国内个人版 qianfan.baidubce.com/
    // .../personal 是两套部署，勿合并。阵容与窗口按 FluxA 产品页模型表
    // （glm-5.2 500k ≠ 国内版千帆平台 1M，国际 team 部署口径，勿按国内预设
    // "修正"）；标注 Coming soon 的 deepseek-v4-pro-0813 / glm-5.3 不收
    name: "FluxA Token Plan",
    websiteUrl: "https://agentmarket.fluxapay.xyz/",
    apiKeyUrl: "https://agentmarket.fluxapay.xyz/marketplace/tokenplans",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://api.baiduqianfan.ai/anthropic/tokenplan/team",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "deepseek-v4-pro[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "deepseek-v4-pro[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "deepseek-v4-pro[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "deepseek-v4-pro[1M]",
      },
    },
    endpointCandidates: [
      "https://api.baiduqianfan.ai/anthropic/tokenplan/team",
    ],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "fluxa",
    icon: "fluxa",
  },
  {
    name: "88API",
    websiteUrl: "https://88api.ai",
    apiKeyUrl: "https://88api.ai/sign-up?aff=HSGY",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.88api.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://api.88api.ai", "https://88api.ai"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "88api",
    icon: "88api",
  },
  {
    name: "APIKEY.FUN",
    websiteUrl: "https://apikey.fan",
    apiKeyUrl: "https://apikey.fan/register?aff=CCSwitch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.apikey.fan",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: [
      "https://api.apikey.fan",
      "https://api.apikey.fun",
      "https://slb.apikey.fun",
    ],
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apikeyfun",
    icon: "apikeyfun",
  },
  {
    name: "9527CODE",
    websiteUrl: "https://9527.codes",
    apiKeyUrl: "https://9527.codes/register?aff=e5zI",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://9527.codes",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: [
      "https://9527.codes",
      "https://api.9527.codes",
      "https://cdn.9527.codes",
    ],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "9527code",
    icon: "9527code",
  },
  {
    name: "ClaudeAPI",
    websiteUrl: "https://www.apito.ai",
    apiKeyUrl: "https://console.apito.ai/agent/register/pQBql2buaqiX3dDS",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://gw.apito.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    icon: "claudeapi",
  },
  {
    name: "Code0",
    websiteUrl: "https://code0.ai",
    apiKeyUrl: "https://code0.ai/agent/register/B2XHxGjGmRvqgznY",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://code0.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    icon: "code0",
  },
  {
    name: "TeamoRouter",
    websiteUrl: "https://teamorouter.cn",
    apiKeyUrl:
      "https://teamorouter.cn/?utm_source=cc_switch&utm_medium=referral&utm_campaign=ai_directory",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.teamorouter.cn",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "teamorouter",
    endpointCandidates: [
      "https://api.teamorouter.cn",
      "https://api.teamorouter.com",
    ],
    icon: "teamorouter",
  },
  {
    name: "PPIO",
    websiteUrl: "https://ppio.com",
    apiKeyUrl: "https://ppio.com/activity/ccswitch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.ppio.com/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "deepseek/deepseek-v4-flash-0731[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "deepseek/deepseek-v4-flash-0731[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "deepseek/deepseek-v4-flash-0731[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "deepseek/deepseek-v4-flash-0731[1M]",
      },
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ppio",
    endpointCandidates: ["https://api.ppio.com/anthropic"],
    modelsUrl: "https://api.ppio.com/openai/v1/models",
    icon: "ppio",
    iconColor: "#2874FF",
  },
  {
    name: "ClaudeCN",
    websiteUrl: "https://claudecn.top",
    apiKeyUrl: "https://claudecn.ai/register?aff=HEL9",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://claudecn.top",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "claudecn",
    icon: "claudecn",
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
      env: {
        ANTHROPIC_BASE_URL: "https://ark.cn-beijing.volces.com/api/plan",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "ark-code-latest",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "ark-code-latest",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "ark-code-latest",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "ark-code-latest",
      },
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "volcengine_agentplan",
    icon: "huoshan",
    iconColor: "#3370FF",
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
      env: {
        ANTHROPIC_BASE_URL: "https://ark.cn-beijing.volces.com/api/coding",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "ark-code-latest",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "ark-code-latest",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "ark-code-latest",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "ark-code-latest",
      },
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "volcengine_codingplan",
    icon: "huoshan",
    iconColor: "#3370FF",
  },
  {
    name: "BytePlus",
    websiteUrl:
      "https://www.byteplus.com/en/product/modelark?utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://www.byteplus.com/en/product/modelark?utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://ark.ap-southeast.bytepluses.com/api/coding",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "ark-code-latest",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "ark-code-latest",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "ark-code-latest",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "ark-code-latest",
      },
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "byteplus",
    icon: "byteplus",
    iconColor: "#3370FF",
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
      env: {
        ANTHROPIC_BASE_URL: "https://ark.cn-beijing.volces.com/api/compatible",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "doubao-seed-2-1-pro-260628",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "doubao-seed-2-1-pro-260628",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "doubao-seed-2-1-pro-260628",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "doubao-seed-2-1-pro-260628",
      },
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "doubaoseed",
    icon: "doubao",
    iconColor: "#3370FF",
  },
  {
    name: "SiliconFlow",
    family: "siliconflow",
    regionKey: "cn",
    websiteUrl: "https://siliconflow.cn",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/YflgU2Ve",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.siliconflow.cn",
        ANTHROPIC_AUTH_TOKEN: "",
        // 国内站 M2.5 已于 2026-09-11 下线，与 Codex/OpenCode 同换 V4 Flash（1M）
        ANTHROPIC_MODEL: "deepseek-ai/DeepSeek-V4-Flash[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "deepseek-ai/DeepSeek-V4-Flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "deepseek-ai/DeepSeek-V4-Flash[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "deepseek-ai/DeepSeek-V4-Flash[1M]",
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
    family: "siliconflow",
    regionKey: "intl",
    websiteUrl: "https://siliconflow.com",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/YflgU2Ve",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.siliconflow.com",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "MiniMaxAI/MiniMax-M3[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "MiniMaxAI/MiniMax-M3[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "MiniMaxAI/MiniMax-M3[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "MiniMaxAI/MiniMax-M3[1M]",
      },
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "siliconflow",
    icon: "siliconflow",
    iconColor: "#000000",
  },
  {
    name: "A6API",
    websiteUrl: "https://www.a6api.com",
    apiKeyUrl: "https://a6api.com/register?aff=AqNr",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.a6api.com",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "a6api",
    icon: "a6api",
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
    name: "Compshare Coding Plan",
    family: "compshare",
    planKey: "codingPlan",
    nameKey: "providerForm.presets.ucloudCoding",
    websiteUrl: "https://www.compshare.cn",
    apiKeyUrl:
      "https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://cp.compshare.cn",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://cp.compshare.cn"],
    category: "aggregator",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "ucloud", // 促销信息 i18n key（复用）
    icon: "ucloud",
    iconColor: "#000000",
  },
  {
    name: "CCSub",
    websiteUrl: "https://www.ccsub.net",
    apiKeyUrl: "https://www.ccsub.net/register?ref=Y6Z8DXEA",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://www.ccsub.net",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ccsub",
    icon: "ccsub",
  },
  {
    name: "SSSAiCode",
    websiteUrl: "https://sssaicodeapi.com",
    apiKeyUrl: "https://sssaicodeapi.com/register?ref=DCP0SM",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://node-hk.sssaicodeapi.com/api",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: [
      "https://node-hk.sssaicodeapi.com/api",
      "https://node-hk.sssaiapi.com/api",
      "https://node-cf.sssaicodeapi.com/api",
    ],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "sssaicode", // 促销信息 i18n key
    icon: "sssaicode",
    iconColor: "#000000",
  },
  {
    name: "SoleAPI",
    websiteUrl: "https://soleapi.com",
    apiKeyUrl: "https://soleapi.com/r/ccswitch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://soleapi.com",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://soleapi.com"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "soleapi",
    icon: "soleapi",
  },
  {
    name: "Micu",
    websiteUrl: "https://www.micuapi.ai",
    apiKeyUrl: "https://www.micuapi.ai/register?aff=aOYQ",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://www.micuapi.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://www.micuapi.ai"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "micu", // 促销信息 i18n key
    icon: "micu",
    iconColor: "#000000",
  },
  {
    name: "RightCode",
    websiteUrl: "https://www.rightapi.ai",
    apiKeyUrl: "https://www.rightapi.ai/register?aff=CCSWITCH",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://www.rightapi.ai/claude",
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
    name: "CrazyRouter",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://cn.crazyrouter.com",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://cn.crazyrouter.com"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "crazyrouter", // 促销信息 i18n key
    icon: "crazyrouter",
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
    icon: "dmxapi",
  },
  {
    name: "SudoCode.chat",
    family: "sudocode",
    websiteUrl: "https://sudocode.chat",
    apiKeyUrl:
      "https://sudocode.chat/sign-up?aff=CC-SWITCH&utm_source=cc-switch&utm_medium=sponsor&utm_campaign=ccswitch",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.sudocode.chat",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: [
      "https://api.sudocode.chat",
      "https://api.sudorelay.com",
    ],
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "sudocode",
    icon: "sudocode",
  },
  {
    name: "SudoCode.us",
    family: "sudocode",
    websiteUrl: "https://sudocode.us",
    apiKeyUrl: "https://sudocode.us",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://sudocode.us",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://sudocode.us", "https://sudocode.run"],
    category: "third_party",
    isPartner: true,
    icon: "sudocode-us",
  },
  {
    name: "XycAi",
    websiteUrl: "https://xycai.us",
    apiKeyUrl: "https://xycai.us/register?aff=Uhu9",
    // 说明：该供应商使用 ANTHROPIC_API_KEY（而非 ANTHROPIC_AUTH_TOKEN）
    apiKeyField: "ANTHROPIC_API_KEY",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://apicdn.xycai.us",
        ANTHROPIC_API_KEY: "",
      },
    },
    endpointCandidates: ["https://apicdn.xycai.us", "https://apicdn.xyc.ai"],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "xycai",
    icon: "xycai",
  },
  {
    name: "Modelflare",
    websiteUrl: "https://modelflare.dev",
    apiKeyUrl:
      "https://modelflare.dev/sign-up?utm_medium=referral&utm_source=ccswitch",
    // 官方 Claude Code 文档（docs.modelflare.dev/guides/clients/claude-code，
    // 2026-10-15 核对）：Anthropic 客户端 base 用站点根（不带 /v1）、Key 填
    // AUTH_TOKEN。主模型与 Sonnet / Opus 跟随 Claude Code 默认；Haiku 档按
    // 官方文档指向 Sonnet：claude-award / claude-tmp-cheap 两个分组没有
    // claude-haiku-5-5（modelflare.dev/api/pricing），Key 在这两组时后台任务
    // 会报找不到模型。origin.modelflare.dev 是面向国内的同一应用（EdgeOne 回源）
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://modelflare.dev",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-sonnet-5-5",
      },
    },
    endpointCandidates: [
      "https://modelflare.dev",
      "https://origin.modelflare.dev",
    ],
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "modelflare",
    icon: "modelflare",
  },
  {
    name: "AllTokens",
    websiteUrl: "https://alltokens.cc",
    apiKeyUrl: "https://alltokens.cc/register",
    // Sub2API 网关：Key 绑定单个分组，须用 Claude 分组的 Key（分组与模型见
    // alltokens.cc/api/v1/model-plaza，2026-10-17 核对）
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://alltokens.cc",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
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
    // 官方 CC Switch 接入文档（alltokens.si/docs/cc-switch/configure）：Anthropic
    // base 用站点根。目录里 Claude ID 用点号（claude-sonnet-5.5，非 5-5），没有
    // Haiku（alltokens.si/api/pricing，2026-10-19），故四档全部写明、Haiku 指向 Sonnet
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://alltokens.si",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "claude-sonnet-5.5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-sonnet-5.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-5.5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-5.5",
      },
    },
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
    // 兔子文档要求 Claude Code 用 ANTHROPIC_API_KEY（x-api-key）而非 AUTH_TOKEN
    apiKeyField: "ANTHROPIC_API_KEY",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.tu-zi.com",
        ANTHROPIC_API_KEY: "",
      },
    },
    endpointCandidates: [
      "https://api.tu-zi.com",
      "https://api.ourzhishi.top",
      "https://api.sydney-ai.com",
      "https://apicdn.tu-zi.com",
    ],
    category: "aggregator",
    icon: "tuzi",
  },
  {
    name: "Amux",
    websiteUrl: "https://amux.ai",
    apiKeyUrl: "https://amux.ai",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.amux.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    icon: "amux",
  },
  {
    name: "AtlasCloud",
    websiteUrl: "https://www.atlascloud.ai/console/coding-plan",
    apiKeyUrl: "https://www.atlascloud.ai/console/coding-plan",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.atlascloud.ai",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "zai-org/glm-5.1",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "zai-org/glm-5.1",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "zai-org/glm-5.1",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "zai-org/glm-5.1",
        CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS: "1",
      },
    },
    endpointCandidates: ["https://api.atlascloud.ai"],
    category: "aggregator",
    icon: "atlascloud",
  },
  {
    // 平台「模型 API 文档」写明 Claude 协议须禁用所有 beta 参数，Claude Code
    // 要设 CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS。模型市场只有 sonnet-5 /
    // opus-5 两款 Claude、没有 Haiku，后台 Haiku 档改指 sonnet-5。
    // AK 只能调已在模型市场订阅的模型。
    name: "Soshow",
    websiteUrl: "https://aimarket.so-show.com",
    apiKeyUrl: "https://aimarket.so-show.com/workbench/access-key",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://maas.so-show.com",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-sonnet-5",
        CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS: "1",
      },
    },
    category: "aggregator",
    icon: "soshow",
  },
  {
    name: "Gemini Native",
    websiteUrl: "https://ai.google.dev/gemini-api",
    apiKeyUrl: "https://aistudio.google.com/app/apikey",
    apiKeyField: "ANTHROPIC_API_KEY",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://generativelanguage.googleapis.com",
        ANTHROPIC_API_KEY: "",
        ANTHROPIC_MODEL: "gemini-3.6-flash",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "gemini-3.6-flash",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "gemini-3.6-flash",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "gemini-3.6-flash",
      },
    },
    category: "third_party",
    apiFormat: "gemini_native",
    endpointCandidates: ["https://generativelanguage.googleapis.com"],
    icon: "gemini",
    iconColor: "#4285F4",
  },
  {
    name: "DeepSeek",
    websiteUrl: "https://platform.deepseek.com",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.deepseek.com/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "deepseek-v4-pro[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "deepseek-flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "deepseek-v4-pro[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "deepseek-v4-pro[1M]",
      },
    },
    category: "cn_official",
    // Anthropic 兼容层挂在 /anthropic 子路径；/models 是根上独立端点
    modelsUrl: "https://api.deepseek.com/models",
    icon: "deepseek",
    iconColor: "#1E88E5",
  },
  {
    name: "OpenCode Go",
    family: "opencode",
    planKey: "coding",
    websiteUrl: "https://opencode.ai/go",
    apiKeyUrl: "https://opencode.ai/go",
    partnerPromotionKey: "opencode_go",
    // Go 网关 /v1/messages 只认 x-api-key（Bearer 被静默忽略），
    // 必须用 ANTHROPIC_API_KEY，不能换回 ANTHROPIC_AUTH_TOKEN。
    // 直连 Anthropic 端点可用除 grok-4.5 外的全部 Go 模型；
    // Chat 组模型（DeepSeek/GLM/Kimi 等）依赖网关服务端格式转换（未见文档承诺）。
    apiKeyField: "ANTHROPIC_API_KEY",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://opencode.ai/zen/go",
        ANTHROPIC_API_KEY: "",
        ANTHROPIC_MODEL: "deepseek-v4-flash[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "deepseek-v4-flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "deepseek-v4-flash[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "deepseek-v4-flash[1M]",
      },
    },
    category: "third_party",
    endpointCandidates: ["https://opencode.ai/zen/go"],
    icon: "opencode",
    iconColor: "#211E1E",
  },
  {
    // Zen 按量网关：Claude 模型原生走 /v1/messages，直连不需要路由。
    // 和 Go 一样只认 x-api-key（Bearer 报 Missing API key）。
    // 免费模型只能在 OpenCode 里用（外部调用 403 FreeTierError），默认不用。
    name: "OpenCode Zen",
    family: "opencode",
    planKey: "payg",
    websiteUrl: "https://opencode.ai/zen",
    apiKeyUrl: "https://opencode.ai/auth",
    apiKeyField: "ANTHROPIC_API_KEY",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://opencode.ai/zen",
        ANTHROPIC_API_KEY: "",
        ANTHROPIC_MODEL: "claude-sonnet-5-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-haiku-4-5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-5-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-5-5",
      },
    },
    category: "aggregator",
    endpointCandidates: ["https://opencode.ai/zen"],
    icon: "opencode",
    iconColor: "#211E1E",
  },
  {
    // 腾讯云 Token Plan 个人版（1823/130060，2026-08-21 版）：通用 + Hy 两
    // 系列共用同一端点与 API Key；Auto 智能路由的调用 ID 是 tc-code-latest。
    // 注意与 TokenHub 按量 API 市场（1823 线，如 Hunyuan 预设的 /v1 端点）
    // 是两条产品线，订阅 Key 只能走 /plan 端点
    name: "Tencent Token Plan",
    family: "tencent",
    planKey: "tokenPlan",
    regionKey: "cn",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://api.lkeap.cloud.tencent.com/plan/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "tc-code-latest",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "tc-code-latest",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "tc-code-latest",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "tc-code-latest",
      },
    },
    category: "cn_official",
    endpointCandidates: ["https://api.lkeap.cloud.tencent.com/plan/anthropic"],
    // /plan/v3/models 真 Key 实测可用（2026-08-31，返回套餐内 19 个模型含
    // 别名）；企业/国际端点无 /models（404），故仅此预设覆写
    modelsUrl: "https://api.lkeap.cloud.tencent.com/plan/v3/models",
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // 国际站（新加坡地域）个人版（intl 1300/81315，2026-08-20 版）：
    // Auto 的调用 ID 是 auto（与国内个人版 tc-code-latest 不同），模型阵容
    // 也与国内不同（无 GLM-5/5.1/Hy3，多 GLM-5.2/MiniMax-M3）。端点用国际站
    // 文档钦定的 tencentcloudmaas.com 域（DNS 实测解析新加坡节点）；国内站
    // 文档对新加坡地域给的是 tokenhub-intl.tencentmaas.com，Key 按站独立、
    // 不跨站通用，故互不作候选
    name: "Tencent Token Plan (Intl)",
    family: "tencent",
    planKey: "tokenPlan",
    regionKey: "intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://tokenhub-intl.tencentcloudmaas.com/plan/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "auto",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "auto",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "auto",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "auto",
      },
    },
    category: "cn_official",
    endpointCandidates: [
      "https://tokenhub-intl.tencentcloudmaas.com/plan/anthropic",
    ],
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // Token Plan 企业版专业套餐（1823/130659，2026-08-25 版）：广州地域端点
    // 为国内站文档钦定的 tencentmaas.com；新加坡地域模型阵容不同且 Key 不
    // 跨站，见 (Intl) 预设
    name: "Tencent Token Plan Enterprise Pro",
    family: "tencent",
    planKey: "enterprisePro",
    regionKey: "cn",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan-e",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://tokenhub.tencentmaas.com/plan/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "auto",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "auto",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "auto",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "auto",
      },
    },
    category: "cn_official",
    // 广州地域为默认端点；国内站企业套餐另可选新加坡地域（1823/130659、
    // 131173 双地域表：tokenhub-intl.tencentmaas.com，需开通新加坡地域，
    // 不支持跨地域调用，故仅作候选端点）
    endpointCandidates: [
      "https://tokenhub.tencentmaas.com/plan/anthropic",
      "https://tokenhub-intl.tencentmaas.com/plan/anthropic",
    ],
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // 国际站企业版专业套餐（intl 1300/81489，2026-08-26 版）：新加坡地域，
    // 模型阵容为广州地域的子集（无 GLM-5/5.1/5-Turbo、Kimi-K2.6、
    // MiniMax-M2.7）
    name: "Tencent Token Plan Enterprise Pro (Intl)",
    family: "tencent",
    planKey: "enterprisePro",
    regionKey: "intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan-e",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://tokenhub-intl.tencentcloudmaas.com/plan/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "auto",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "auto",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "auto",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "auto",
      },
    },
    category: "cn_official",
    // 新加坡地域为默认端点；国际站企业套餐另可选广州地域（1300/81489、
    // 81490 双地域表：tokenhub.tencentcloudmaas.com，需开通广州地域，
    // 不支持跨地域调用，故仅作候选端点）
    endpointCandidates: [
      "https://tokenhub-intl.tencentcloudmaas.com/plan/anthropic",
      "https://tokenhub.tencentcloudmaas.com/plan/anthropic",
    ],
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // Token Plan 企业版轻享套餐（1823/131173，2026-08-28 版）：仅 Auto 模型
    name: "Tencent Token Plan Enterprise Lite",
    family: "tencent",
    planKey: "enterpriseLite",
    regionKey: "cn",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan-e",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://tokenhub.tencentmaas.com/plan/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "auto",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "auto",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "auto",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "auto",
      },
    },
    category: "cn_official",
    // 广州地域为默认端点；国内站企业套餐另可选新加坡地域（1823/130659、
    // 131173 双地域表：tokenhub-intl.tencentmaas.com，需开通新加坡地域，
    // 不支持跨地域调用，故仅作候选端点）
    endpointCandidates: [
      "https://tokenhub.tencentmaas.com/plan/anthropic",
      "https://tokenhub-intl.tencentmaas.com/plan/anthropic",
    ],
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    // 国际站企业版轻享套餐（intl 1300/81490）：新加坡地域（资源调度范围为
    // Global），仅 Auto 模型
    name: "Tencent Token Plan Enterprise Lite (Intl)",
    family: "tencent",
    planKey: "enterpriseLite",
    regionKey: "intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan-e",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://tokenhub-intl.tencentcloudmaas.com/plan/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "auto",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "auto",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "auto",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "auto",
      },
    },
    category: "cn_official",
    // 新加坡地域为默认端点；国际站企业套餐另可选广州地域（1300/81489、
    // 81490 双地域表：tokenhub.tencentcloudmaas.com，需开通广州地域，
    // 不支持跨地域调用，故仅作候选端点）
    endpointCandidates: [
      "https://tokenhub-intl.tencentcloudmaas.com/plan/anthropic",
      "https://tokenhub.tencentcloudmaas.com/plan/anthropic",
    ],
    icon: "tencent",
    iconColor: "#0052D9",
  },
  {
    name: "Zhipu GLM",
    family: "zhipu",
    regionKey: "cn",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://open.bigmodel.cn/api/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "glm-5.3[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "glm-5.3[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.3[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "glm-5.3[1M]",
      },
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
  },
  {
    name: "Zhipu GLM en",
    family: "zhipu",
    regionKey: "intl",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.z.ai/api/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "glm-5.3[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "glm-5.3[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.3[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "glm-5.3[1M]",
      },
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
  },
  {
    name: "Baidu Qianfan Coding Plan",
    family: "baidu-qianfan",
    planKey: "codingPlan",
    websiteUrl: "https://cloud.baidu.com/product/qianfan_modelbuilder",
    apiKeyUrl:
      "https://console.bce.baidu.com/qianfan/ais/console/applicationConsole/application",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://qianfan.baidubce.com/anthropic/coding",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "qianfan-code-latest",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "qianfan-code-latest",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "qianfan-code-latest",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "qianfan-code-latest",
      },
    },
    category: "cn_official",
    endpointCandidates: ["https://qianfan.baidubce.com/anthropic/coding"],
    icon: "baidu",
    iconColor: "#2932E1",
  },
  {
    // Token Plan 个人版：2026-07-13 起替代 Coding Plan 发售（存量 Coding
    // Plan 可用至到期，旧预设保留）。模型=官方 Claude Code 接入页
    // （2026-07-30 版）全角色 deepseek-v4-pro；Key 是订阅页专属 Key
    name: "Baidu Qianfan Token Plan",
    family: "baidu-qianfan",
    planKey: "tokenPlan",
    websiteUrl: "https://cloud.baidu.com/product/codingplan.html",
    apiKeyUrl: "https://console.bce.baidu.com/qianfan/resource/token-plan",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://qianfan.baidubce.com/anthropic/tokenplan/personal",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "deepseek-v4-pro[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "deepseek-v4-pro[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "deepseek-v4-pro[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "deepseek-v4-pro[1M]",
      },
    },
    category: "cn_official",
    endpointCandidates: [
      "https://qianfan.baidubce.com/anthropic/tokenplan/personal",
    ],
    icon: "baidu",
    iconColor: "#2932E1",
  },
  {
    name: "千问AI平台",
    family: "qianwen",
    planKey: "payg",
    websiteUrl: "https://platform.qianwenai.com/?utm_content=g_20000002971",
    apiKeyUrl:
      "https://platform.qianwenai.com/home/api-keys?utm_content=g_20000002972",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://dashscope.aliyuncs.com/apps/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "qwen3.8-max[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "qwen3.8-flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "qwen3.7-plus[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "qwen3.8-max[1M]",
        // 模型 id 非 claude-* 时 Claude Code 按 200K 默认窗口处理：各档带
        // [1M]（表单预勾，优先于下面的值）；这里保留官方钉的值兜底取消
        // 勾选或换成未勾模型的档位：qwen3.8 系 context_window = 983616
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "983616",
      },
    },
    category: "cn_official",
    icon: "qianwenai",
    iconColor: "#624AFF",
  },
  {
    name: "千问AI平台 Coding Plan",
    family: "qianwen",
    planKey: "codingPlan",
    websiteUrl: "https://bailian.console.aliyun.com",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://coding.dashscope.aliyuncs.com/apps/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "cn_official",
    icon: "qianwenai",
    iconColor: "#624AFF",
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
      env: {
        ANTHROPIC_BASE_URL:
          "https://token-plan.cn-beijing.maas.aliyuncs.com/apps/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "qwen3.8-max[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "qwen3.8-flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "qwen3.7-plus[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "qwen3.8-max[1M]",
        // 官方 Token Plan 配置同样钉窗口：qwen3.8 系 context_window = 983616；
        // 各档带 [1M] 时以标记为准，这里兜底未勾 1M 的档位
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "983616",
      },
    },
    category: "cn_official",
    icon: "qianwenai",
    iconColor: "#624AFF",
  },
  // ===== QwenCloud（DashScope 国际站）=====
  // 与上面国内百炼是两套独立站点：域名、控制台、密钥互不通用。
  // 三条线各有专属 base_url 与专属 API Key，官方文档明示密钥类型与
  // base_url 不匹配会 401，因此拆成三个预设而非共用一条加候选地址。
  {
    name: "QwenCloud",
    family: "qwencloud",
    planKey: "payg",
    websiteUrl: "https://home.qwencloud.com/?utm_content=g_20000002974",
    apiKeyUrl: "https://home.qwencloud.com/api-keys?utm_content=g_20000002975",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://dashscope-intl.aliyuncs.com/apps/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "qwen3.8-max[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "qwen3.8-flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "qwen3.7-plus[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "qwen3.8-max[1M]",
      },
    },
    category: "cn_official",
    icon: "qwencloud",
    iconColor: "#6336E7",
  },
  {
    name: "QwenCloud For Coding",
    family: "qwencloud",
    planKey: "coding",
    websiteUrl: "https://www.qwencloud.com",
    apiKeyUrl: "https://home.qwencloud.com/api-keys",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://coding-intl.dashscope.aliyuncs.com/apps/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "qwen3.7-plus[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "qwen3.7-plus[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "qwen3.7-plus[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "qwen3.7-plus[1M]",
      },
    },
    category: "cn_official",
    icon: "qwencloud",
    iconColor: "#6336E7",
  },
  {
    name: "QwenCloud Token Plan",
    family: "qwencloud",
    planKey: "tokenPlan",
    websiteUrl:
      "https://www.qwencloud.com/pricing/token-plan?utm_content=g_20000002980",
    apiKeyUrl: "https://home.qwencloud.com/api-keys?utm_content=g_20000002981",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "qwen3.8-max[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "qwen3.8-flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "qwen3.7-plus[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "qwen3.8-max[1M]",
        // 官方 Claude Code 配置钉的窗口：qwen3.8 系 context_window = 983616；
        // 各档带 [1M] 时以标记为准，这里兜底未勾 1M 的档位
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "983616",
      },
    },
    category: "cn_official",
    icon: "qwencloud",
    iconColor: "#6336E7",
  },
  {
    name: "StepFun",
    family: "stepfun",
    regionKey: "cn",
    websiteUrl: "https://platform.stepfun.com/step-plan",
    apiKeyUrl: "https://platform.stepfun.com/interface-key",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.stepfun.com/step_plan",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "step-3.5-flash-2603",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "step-3.5-flash-2603",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "step-3.5-flash-2603",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "step-3.5-flash-2603",
      },
    },
    category: "cn_official",
    endpointCandidates: ["https://api.stepfun.com/step_plan"],
    icon: "stepfun",
    iconColor: "#16D6D2",
  },
  {
    name: "StepFun en",
    family: "stepfun",
    regionKey: "intl",
    websiteUrl: "https://platform.stepfun.ai/step-plan",
    apiKeyUrl: "https://platform.stepfun.ai/interface-key",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.stepfun.ai/step_plan",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "step-3.5-flash-2603",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "step-3.5-flash-2603",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "step-3.5-flash-2603",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "step-3.5-flash-2603",
      },
    },
    category: "cn_official",
    endpointCandidates: ["https://api.stepfun.ai/step_plan"],
    icon: "stepfun",
    iconColor: "#16D6D2",
  },
  {
    name: "ModelScope",
    websiteUrl: "https://modelscope.cn",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api-inference.modelscope.cn",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "ZhipuAI/GLM-5.2[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "ZhipuAI/GLM-5.2[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "ZhipuAI/GLM-5.2[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "ZhipuAI/GLM-5.2[1M]",
      },
    },
    category: "aggregator",
    icon: "modelscope",
    iconColor: "#624AFF",
  },
  {
    name: "StreamLake",
    family: "streamlake",
    regionKey: "cn",
    websiteUrl: "https://www.streamlake.com",
    apiKeyUrl: "https://console.streamlake.com/console/wanqing/api-key",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://wanqing.streamlakeapi.com/api/gateway/v1/endpoints/kat-coder-pro-v2.5/claude-code-proxy",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "kat-coder-pro-v2.5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "kat-coder-pro-v2.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "kat-coder-pro-v2.5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "kat-coder-pro-v2.5",
      },
    },
    category: "cn_official",
    icon: "streamlake",
  },
  {
    name: "StreamLake en",
    family: "streamlake",
    regionKey: "intl",
    websiteUrl: "https://console.streamlake.ai",
    apiKeyUrl: "https://console.streamlake.ai/console/api-key",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://vanchin.streamlake.ai/api/gateway/v1/endpoints/kat-coder-pro-v2.5/claude-code-proxy",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "kat-coder-pro-v2.5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "kat-coder-pro-v2.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "kat-coder-pro-v2.5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "kat-coder-pro-v2.5",
      },
    },
    category: "cn_official",
    icon: "streamlake",
  },
  {
    name: "Longcat",
    websiteUrl: "https://longcat.chat/platform",
    apiKeyUrl: "https://longcat.chat/platform/api_keys",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.longcat.chat/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "LongCat-2.0[1M]",
        ANTHROPIC_SMALL_FAST_MODEL: "LongCat-2.0",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "LongCat-2.0[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "LongCat-2.0[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "LongCat-2.0[1M]",
        CLAUDE_CODE_MAX_OUTPUT_TOKENS: "131072",
      },
    },
    category: "cn_official",
    icon: "longcat",
    iconColor: "#29E154",
  },
  {
    name: "MiniMax",
    family: "minimax",
    regionKey: "cn",
    websiteUrl: "https://platform.minimax.cn",
    apiKeyUrl: "https://platform.minimax.cn/subscribe/token-plan",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.minimax.cn/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        CLAUDE_CODE_AUTO_COMPACT_WINDOW: "1000000",
        ANTHROPIC_MODEL: "MiniMax-M3.1-Flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "MiniMax-M3.1-Flash[1M]",
        // M3 stays selectable as Opus; a role slot also works through the local route
        ANTHROPIC_DEFAULT_OPUS_MODEL: "MiniMax-M3[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "MiniMax-M3.1-Flash[1M]",
      },
    },
    category: "cn_official",
    theme: {
      backgroundColor: "#f64551",
      textColor: "#FFFFFF",
    },
    icon: "minimax",
    iconColor: "#FF6B6B",
  },
  {
    name: "MiniMax en",
    family: "minimax",
    regionKey: "intl",
    websiteUrl: "https://platform.minimax.io",
    apiKeyUrl: "https://platform.minimax.io/subscribe/coding-plan",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.minimax.io/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        CLAUDE_CODE_AUTO_COMPACT_WINDOW: "1000000",
        ANTHROPIC_MODEL: "MiniMax-M3.1-Flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "MiniMax-M3.1-Flash[1M]",
        // M3 stays selectable as Opus; a role slot also works through the local route
        ANTHROPIC_DEFAULT_OPUS_MODEL: "MiniMax-M3[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "MiniMax-M3.1-Flash[1M]",
      },
    },
    category: "cn_official",
    theme: {
      backgroundColor: "#f64551",
      textColor: "#FFFFFF",
    },
    icon: "minimax",
    iconColor: "#FF6B6B",
  },
  {
    name: "BaiLing",
    websiteUrl: "https://developer.ant-ling.com/zh-CN/docs/",
    apiKeyUrl: "https://chat.ant-ling.com/open",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.ant-ling.com/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "Ling-2.6-1T",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "Ling-2.6-1T",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "Ling-2.6-1T",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "Ling-2.6-1T",
      },
    },
    category: "cn_official",
    icon: "bailing",
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
    name: "CherryIN",
    websiteUrl: "https://open.cherryin.ai",
    apiKeyUrl: "https://open.cherryin.ai/console/token",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://open.cherryin.net",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "anthropic/claude-sonnet-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "anthropic/claude-haiku-4.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "anthropic/claude-sonnet-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "anthropic/claude-opus-5",
      },
    },
    category: "aggregator",
    endpointCandidates: ["https://open.cherryin.net"],
    icon: "cherryin",
  },
  {
    name: "RelaxyCode",
    websiteUrl: "https://www.relaxycode.com",
    apiKeyUrl: "https://www.relaxycode.com/register",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://www.relaxycode.com",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "third_party",
    icon: "relaxcode",
  },
  {
    name: "E-FlowCode",
    websiteUrl: "https://e-flowcode.cc",
    apiKeyUrl: "https://e-flowcode.cc",
    settingsConfig: {
      env: {
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_BASE_URL: "https://e-flowcode.cc",
        ENABLE_TOOL_SEARCH: "true",
      },
    },
    category: "third_party",
    endpointCandidates: ["https://e-flowcode.cc"],
    icon: "eflowcode",
    iconColor: "#000000",
  },
  {
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://openrouter.ai/api",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "anthropic/claude-sonnet-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "anthropic/claude-haiku-4.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "anthropic/claude-sonnet-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "anthropic/claude-opus-5",
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
        ANTHROPIC_MODEL: "anthropic/claude-sonnet-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "anthropic/claude-haiku-4.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "anthropic/claude-sonnet-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "anthropic/claude-opus-5",
      },
    },
    category: "aggregator",
    endpointCandidates: ["https://api.therouter.ai"],
    icon: "therouter",
  },
  {
    name: "Baseten",
    websiteUrl: "https://www.baseten.co",
    apiKeyUrl: "https://app.baseten.co/settings/api_keys",
    // Anthropic Messages 端点 /v1/messages（docs.baseten.co/development/
    // model-apis/overview，2026-10-14 核对）官方标注 beta，生产推荐 Chat
    // Completions；冒烟不过就改 apiFormat: "openai_chat"，地址不变。鉴权只认
    // Authorization: Bearer / Api-Key，不认 x-api-key（Messages OpenAPI 原文），
    // 默认的 AUTH_TOKEN 正好发 Bearer。模型 ID 是固定版本、无 latest 别名；
    // haiku 用小模型 GLM-5.3-Flash（-Fast 是同权重独享算力的高价版）。
    // 根路径 + /v1/models 即模型列表（无 Key 401），不覆写 modelsUrl
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://inference.baseten.co",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "zai-org/GLM-5.3[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "zai-org/GLM-5.3-Flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "zai-org/GLM-5.3[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "moonshotai/Kimi-K3[1M]",
      },
    },
    category: "aggregator",
    icon: "baseten",
    endpointCandidates: ["https://inference.baseten.co"],
  },
  {
    name: "Novita AI",
    websiteUrl: "https://novita.ai",
    apiKeyUrl: "https://novita.ai",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.novita.ai/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "zai-org/glm-5.1",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "zai-org/glm-5.1",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "zai-org/glm-5.1",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "zai-org/glm-5.1",
      },
    },
    category: "aggregator",
    endpointCandidates: ["https://api.novita.ai/anthropic"],
    // Anthropic 兼容层在 /anthropic 子路径，OpenAI 侧却在 /openai/v1；剥后缀
    // 后的根路径没有 /models（实测 404），通用候选够不到，故覆写
    modelsUrl: "https://api.novita.ai/openai/v1/models",
    icon: "novita",
    iconColor: "#000000",
  },
  {
    name: "Ollama Cloud",
    websiteUrl: "https://ollama.com",
    apiKeyUrl: "https://ollama.com/settings/keys",
    // 官方 Claude Code 直连文档（docs.ollama.com/integrations/claude-code，
    // 2026-10-14 核对）：base 为 https://ollama.com、Key 填 AUTH_TOKEN、
    // ANTHROPIC_API_KEY 置空。云端只认 Authorization: Bearer，不认单独的
    // x-api-key（docs.ollama.com/api/anthropic-compatibility），默认的
    // AUTH_TOKEN 正好是 Bearer。兼容层不支持 cache_control 与 count_tokens。
    // 模型 ID 调 API 时不带 :cloud 后缀（docs.ollama.com/cloud）
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://ollama.com",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_API_KEY: "",
        ANTHROPIC_MODEL: "glm-5.3[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "glm-5.3-flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.3[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "kimi-k3[1M]",
      },
    },
    category: "cloud_provider",
    // api.ollama.com / www.ollama.com 都 301/308 跳回 ollama.com，不另列候选
    endpointCandidates: ["https://ollama.com"],
    icon: "ollama",
  },
  {
    name: "Ollama",
    websiteUrl: "https://ollama.com",
    // 本机 Ollama（docs.ollama.com/integrations/claude-code，2026-10-14 核对）：
    // /v1/messages 自 v0.14.0 起支持，服务端不校验 Key，官方写法是
    // AUTH_TOKEN=ollama、ANTHROPIC_API_KEY 置空；默认只监听 127.0.0.1:11434
    //（OLLAMA_HOST 可改）。模型由用户自己 ollama pull：qwen3.5 是官方示例，
    // 换成自己已下载的模型，或用「获取模型」从 /v1/models 拉取（id 带 :latest
    // 等 tag）。实际窗口受服务端限制：显存低于 24 GiB 时默认只有 4k，官方要求
    // 编程工具至少 64k（OLLAMA_CONTEXT_LENGTH=64000 ollama serve，
    // docs.ollama.com/context-length）。兼容层不支持 cache_control、
    // count_tokens 与 tool_choice（docs.ollama.com/api/anthropic-compatibility）
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "http://localhost:11434",
        ANTHROPIC_AUTH_TOKEN: "ollama",
        ANTHROPIC_API_KEY: "",
        ANTHROPIC_MODEL: "qwen3.5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "qwen3.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "qwen3.5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "qwen3.5",
      },
    },
    category: "local",
    endpointCandidates: ["http://localhost:11434", "http://127.0.0.1:11434"],
    icon: "ollama",
  },
  {
    name: "LM Studio",
    websiteUrl: "https://lmstudio.ai",
    // 本机 LM Studio（lmstudio.ai/docs/integrations/claude-code，2026-10-14
    // 核对）：Anthropic 兼容 /v1/messages 自 0.4.1 起支持，base 不带 /v1。默认
    // 不鉴权，官方写法 AUTH_TOKEN=lmstudio，预填该占位值；开启 Require
    // Authentication 后换成 App 里创建的 API Token（x-api-key 与 Bearer 都认，
    // lmstudio.ai/docs/developer/core/authentication）。官方另建议
    // CLAUDE_CODE_ATTRIBUTION_HEADER=0，它不是关键 / 独有字段，写进预设不生效，
    // 需自行加到全局设置。模型由用户自己下载：openai/gpt-oss-20b 是官方示例，
    // 换成自己已下载的模型 ID，或用「获取模型」从 /v1/models 拉取。已知问题
    //（lmstudio-bug-tracker #2164、#2220，仍未关）：0.4.19 Build 2 起 Qwen 停在
    // 一句话、0.4.20 起 gpt-oss-120b 报 grammar 400，撞上时可改 openai_chat +
    // http://localhost:1234/v1 走路由
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "http://localhost:1234",
        ANTHROPIC_AUTH_TOKEN: "lmstudio",
        ANTHROPIC_MODEL: "openai/gpt-oss-20b",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "openai/gpt-oss-20b",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "openai/gpt-oss-20b",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "openai/gpt-oss-20b",
        // 模型 id 非 claude-* 时 Claude Code 按 200K 默认窗口处理，钉住
        // gpt-oss-20b 的原生值 131072。实际窗口受 LM Studio 加载模型时的
        // context length 限制（UI 设置或 lms load --context-length），官方要求
        // 至少约 25K；加载窗口更小时此值要一起调小
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "131072",
      },
    },
    category: "local",
    icon: "lmstudio",
    endpointCandidates: ["http://localhost:1234", "http://127.0.0.1:1234"],
  },
  {
    name: "llama.cpp",
    websiteUrl: "https://github.com/ggml-org/llama.cpp",
    // 本机 llama-server（github.com/ggml-org/llama.cpp/blob/master/tools/server/
    // README.md，2026-10-14 核对）：Anthropic 兼容 /v1/messages 自 b7187 起支持
    //（PR #17570），只有带 /v1 的路由，base 不带 /v1；工具调用需要的 jinja 自
    // b7170 起默认开启，不用再加 --jinja。默认端口自 b11521（PR #30159，
    // 2026-10-09）起为 9931，v0.6.0 及更早、Homebrew 与 Docker 镜像仍是 8080，
    // 见候选地址；默认只监听 IPv4 的 127.0.0.1，故不写 localhost。不带
    // --api-key 启动时不校验 Key，预填占位值 no-key（同 README 示例）；带了
    // --api-key 就填那个值（Bearer 与 x-api-key 都认）。server 会归一化
    // x-anthropic-billing-header 里每次变化的 cch=，前缀缓存不受影响（PR #21793）。
    // 模型由用户自己下载：默认取 ggml-org 官方 GGUF 的 Qwen3-Coder 30B A3B
    //（llama-server -hf 拉取后 /v1/models 的 id 即此值），换成自己已下载的模型，
    // 或用「获取模型」从 /v1/models 拉取；单模型启动时 model 字段被忽略，
    // router 模式（启动不带 -m / -hf）须与 /v1/models 的 id 一致。模型 id 非
    // claude-* 时 Claude Code 按 200K 默认窗口处理，而本地实际窗口受 -c / --fit /
    // -np 限制（以 /v1/models 的 meta.n_ctx 为准），更小时自行加
    // CLAUDE_CODE_MAX_CONTEXT_TOKENS 钉成实际值
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "http://127.0.0.1:9931",
        ANTHROPIC_AUTH_TOKEN: "no-key",
        ANTHROPIC_MODEL: "ggml-org/Qwen3-Coder-30B-A3B-Instruct-Q8_0-GGUF",
        ANTHROPIC_DEFAULT_HAIKU_MODEL:
          "ggml-org/Qwen3-Coder-30B-A3B-Instruct-Q8_0-GGUF",
        ANTHROPIC_DEFAULT_SONNET_MODEL:
          "ggml-org/Qwen3-Coder-30B-A3B-Instruct-Q8_0-GGUF",
        ANTHROPIC_DEFAULT_OPUS_MODEL:
          "ggml-org/Qwen3-Coder-30B-A3B-Instruct-Q8_0-GGUF",
      },
    },
    category: "local",
    icon: "llamacpp",
    endpointCandidates: ["http://127.0.0.1:9931", "http://127.0.0.1:8080"],
  },
  {
    name: "vLLM",
    websiteUrl: "https://vllm.ai",
    // 本机 vllm serve（docs.vllm.ai/en/stable/serving/integrations/claude_code，
    // v0.31.0 源码，2026-10-14 核对）：Anthropic 兼容 /v1/messages 自 v0.11.1
    // 起支持，与 OpenAI 接口同端口，base 不带 /v1；Claude Code 2.1.154 起在
    // messages 里带 system 角色，v0.23.0 之前报 400（PR #44283），故要求
    // vLLM ≥ 0.23.0；0.17.1 之后会自动剥掉 Claude Code 注入的逐请求 hash，
    // 前缀缓存不受影响，不用设 CLAUDE_CODE_ATTRIBUTION_HEADER。默认端口 8000，
    // 不带 --host 时监听全部 IPv4 网卡，建议加 --host 127.0.0.1。不带 --api-key
    // 时不校验 Key，预填官方占位值 dummy；带了就填那个值（只认 Bearer，不认
    // x-api-key，默认的 AUTH_TOKEN 正好是 Bearer）。工具调用要启动时加
    // --enable-auto-tool-choice 与 --tool-call-parser，推荐命令（Qwen 模型卡）：
    // vllm serve Qwen/Qwen3.6-27B --max-model-len 262144 --reasoning-parser qwen3
    // --enable-auto-tool-choice --tool-call-parser qwen3_coder。兼容层不支持
    // PDF document 块，cache_control 被忽略。模型由用户自己下载：默认取 vLLM
    // Codex 接入页的示例 Qwen/Qwen3.6-27B，模型 ID 须与 /v1/models 返回的一致
    //（没传 --served-model-name 时就是 HF 仓库名），换成自己的模型，或用
    // 「获取模型」从 /v1/models 拉取。模型 id 非 claude-* 时 Claude Code 按 200K
    // 默认窗口处理，而本地实际窗口受 --max-model-len 限制，更小时自行加
    // CLAUDE_CODE_MAX_CONTEXT_TOKENS 钉成实际值
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "http://localhost:8000",
        ANTHROPIC_AUTH_TOKEN: "dummy",
        ANTHROPIC_MODEL: "Qwen/Qwen3.6-27B",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "Qwen/Qwen3.6-27B",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "Qwen/Qwen3.6-27B",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "Qwen/Qwen3.6-27B",
      },
    },
    category: "local",
    icon: "vllm",
    endpointCandidates: ["http://localhost:8000", "http://127.0.0.1:8000"],
  },
  {
    name: "Meta Model API",
    websiteUrl: "https://dev.meta.ai",
    // Key 在 dev.meta.ai 登录后的「API keys」标签页（dev.meta.ai/docs/
    // authentication.md，2026-10-14 核对），没有独立的 Key 页面
    apiKeyUrl: "https://dev.meta.ai",
    // 官方 Claude Code 接入文档（dev.meta.ai/docs/coding-agents.md，2026-10-14
    // 核对）原样：base 不带 /v1（Claude Code 自己拼 /v1/messages），只认
    // Authorization: Bearer，Key 填 AUTH_TOKEN 而非 ANTHROPIC_API_KEY
    //（x-api-key）；五个模型变量全部钉到 Muse，主循环/opus 用 Wonder、
    // sonnet/haiku/子代理用 Spark 1.3。两个 -contributor 版本会拿用户内容训练
    // 模型，禁收。根路径 + /v1/models 即模型列表（无 Key 401），不覆写 modelsUrl
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.meta.ai",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "muse-wonder-1.0[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "muse-wonder-1.0[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "muse-spark-1.3[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "muse-spark-1.3[1M]",
        CLAUDE_CODE_SUBAGENT_MODEL: "muse-spark-1.3[1M]",
        // 非第一方 host 默认关闭 MCP 工具搜索，官方要求打开
        ENABLE_TOOL_SEARCH: "true",
        // 模型 id 非 claude-* 时 Claude Code 按 200K 默认窗口处理：两款模型
        // 窗口均为 1048576（dev.meta.ai/docs/models.md），各档带 [1M]（表单
        // 预勾）；这里兜底未勾 1M 的档位
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "1048576",
      },
    },
    category: "cn_official",
    endpointCandidates: ["https://api.meta.ai"],
    icon: "meta",
  },
  {
    name: "DeepInfra",
    websiteUrl: "https://deepinfra.com",
    apiKeyUrl: "https://deepinfra.com/dash/api_keys",
    // 官方 Claude Code 接入文档（docs.deepinfra.com/integrations/anthropic.md，
    // 2026-10-14 核对）：base 为 /anthropic、不带 /v1，Bearer 与 x-api-key 都
    // 收，默认的 AUTH_TOKEN 即可。档位与 ANTHROPIC_MODEL 照抄官方表：opus
    // V4 Pro 0813、sonnet/主循环 V4 Flash 0731、haiku Ling-3.0-flash；不带日期
    // 的 V4 Pro/Flash 是已被取代的早期预览，不用。官方另写的 fable、子代理与
    // 最大输出变量照仓库惯例不加。/anthropic/v1/models 404，通用候选剥掉
    // /anthropic 后的 /v1/models 公开可读，不覆写 modelsUrl
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.deepinfra.com/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "deepseek-ai/DeepSeek-V4-Flash-0731[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "inclusionAI/Ling-3.0-flash",
        ANTHROPIC_DEFAULT_SONNET_MODEL:
          "deepseek-ai/DeepSeek-V4-Flash-0731[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "deepseek-ai/DeepSeek-V4-Pro-0813[1M]",
      },
    },
    category: "aggregator",
    icon: "deepinfra",
    endpointCandidates: ["https://api.deepinfra.com/anthropic"],
  },
  {
    name: "Vercel AI Gateway",
    websiteUrl: "https://vercel.com/ai-gateway",
    apiKeyUrl:
      "https://vercel.com/d?to=%2F%5Bteam%5D%2F%7E%2Fai-gateway%2Fapi-keys",
    // 官方 Claude Code 接入文档（vercel.com/docs/ai-gateway/coding-agents/
    // claude-code，2026-10-14 核对）：base 为 /claude-code、不带 /v1（SDK 自拼
    // /v1/messages，再加即 404），Key 放 AUTH_TOKEN 发 Bearer，并要求把
    // ANTHROPIC_API_KEY 置空。模型 ID 带 anthropic/ 前缀，档位取平台目录同日
    // 最新的 Sonnet / Opus / Haiku 5.5（均 1M）。官方 CLI 另写的
    // CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY 照仓库惯例不加。
    // /claude-code/v1/models 返回带 claude-code/…[1m] 显示前后缀的 ID，改用
    // /coding-agent/v1/models（公开可读、ID 干净、只含语言模型）
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://ai-gateway.vercel.sh/claude-code",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_API_KEY: "",
        ANTHROPIC_MODEL: "anthropic/claude-sonnet-5.5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "anthropic/claude-haiku-5.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "anthropic/claude-sonnet-5.5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "anthropic/claude-opus-5.5",
      },
    },
    category: "aggregator",
    endpointCandidates: ["https://ai-gateway.vercel.sh/claude-code"],
    modelsUrl: "https://ai-gateway.vercel.sh/coding-agent/v1/models",
    icon: "vercel",
  },
  {
    name: "Nebius Token Factory",
    websiteUrl: "https://nebius.com/token-factory",
    apiKeyUrl: "https://tokenfactory.nebius.com/project/api-keys",
    // 原 Nebius AI Studio。官方 OpenAPI（api.tokenfactory.nebius.com/
    // openapi.json，2026-10-14 核对）只有 /v1/chat/completions、/v1/responses
    // 等 OpenAI 路径，鉴权只声明 HTTPBearer；POST /v1/messages 实测 404，没有
    // Anthropic 端点，只能走 openai_chat 路由。base 不带 /v1，适配器自拼
    // /v1/chat/completions（同 Nvidia）。模型取官方公开目录
    // tokenfactory.nebius.com/api/public/models_info 同日快照，用固定版本 ID：
    // 主力 GLM-5.3、haiku 档 GLM-5.3-Flash（均 1024000 窗口）。通用候选的
    // /v1/models 存在（无 Key 401），不覆写 modelsUrl
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.tokenfactory.nebius.com",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "zai-org/GLM-5.3[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "zai-org/GLM-5.3-Flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "zai-org/GLM-5.3[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "zai-org/GLM-5.3[1M]",
      },
    },
    category: "aggregator",
    icon: "nebius",
    apiFormat: "openai_chat",
    endpointCandidates: ["https://api.tokenfactory.nebius.com"],
  },
  {
    name: "Groq",
    websiteUrl: "https://groq.com",
    apiKeyUrl: "https://console.groq.com/keys",
    // OpenAI 兼容 base 为 https://api.groq.com/openai/v1（console.groq.com/
    // docs/openai，2026-10-14 核对），Bearer 鉴权。/anthropic/v1/models 已在线
    // 但 POST /anthropic/v1/messages 实测 404，官方文档与 changelog 均未提
    // Anthropic 端点，只能走 openai_chat 路由；适配器拼出 /openai/v1/v1/... 后
    // 去重 /v1/v1，落到 /openai/v1/chat/completions。base 以 /v1 结尾，模型列表
    // 自动探测 {base}/models（无 Key 401），不写 modelsUrl。编程可用的自助
    // Production 模型只有 gpt-oss 两款（console.groq.com/docs/models 同日）：
    // 主力 120b、haiku 档 20b；qwen3.8-27b 为 Preview 且最大输出仅 16384，不收
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.groq.com/openai/v1",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "openai/gpt-oss-120b",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "openai/gpt-oss-20b",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "openai/gpt-oss-120b",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "openai/gpt-oss-120b",
        // 模型 id 非 claude-* 时 Claude Code 按 200K 默认窗口处理，必须显式
        // 钉住官方值：gpt-oss 两款 context window = 131072
        //（console.groq.com/docs/model/openai/gpt-oss-120b）
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "131072",
      },
    },
    category: "aggregator",
    icon: "groq",
    apiFormat: "openai_chat",
    endpointCandidates: ["https://api.groq.com/openai/v1"],
  },
  {
    name: "SambaNova",
    websiteUrl: "https://sambanova.ai",
    apiKeyUrl: "https://cloud.sambanova.ai/apis",
    // 官方 Claude Code 接入文档 Option A 直连（docs.sambanova.ai/docs/en/
    // integrations/claude-code，2026-10-14 核对）：base 为 https://api.sambanova.ai、
    // 不带 /v1（文档排错表写明带 /v1 会 404；实测 /v1/v1/messages 404、
    // /v1/messages 无 Key 401）。Bearer 与 x-api-key 都收，默认的 AUTH_TOKEN 即可。
    // 模型照官方文档全档 MiniMax-M3（Preview，官方说可能短期下架，届时改用
    // Production 的 gpt-oss-120b）；官方强调 haiku 档也要设，否则后台任务失败。
    // 平台声明窗口 1048576，各档带 [1M]（表单预勾）。[1M] 会让 Claude
    // Code 多发 context-1m beta 头，但同一请求本就带着十来个 beta 头
    //（2.1.296 本地模拟接口实测，2026-10-15），多它一个不改变兼容性。
    // OpenAPI 写明 v1 的 thinking
    // enabled/adaptive 返回 400，与官方文档说可直连矛盾，未经真 Key 实测。
    // DeepSeek-V3.1 最大输出仅 7168，低于 Claude Code 默认 max_tokens，不用。
    // /v1/models 公开可读，默认探测即可，不写 modelsUrl
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.sambanova.ai",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "MiniMax-M3[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "MiniMax-M3[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "MiniMax-M3[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "MiniMax-M3[1M]",
      },
    },
    category: "cloud_provider",
    icon: "sambanova",
    endpointCandidates: ["https://api.sambanova.ai"],
  },
  {
    name: "DigitalOcean",
    websiteUrl: "https://www.digitalocean.com/products/ai-platform",
    apiKeyUrl: "https://cloud.digitalocean.com/model-studio/manage-keys",
    // 原 Gradient，现名 DigitalOcean AI Platform，推理服务文档里叫 Inference。
    // 官方 Coding Agents 文档（docs.digitalocean.com/products/inference/how-to/
    // use-with-coding-agents，2026-10-14 核对）：base 为 https://inference.do-ai.run、
    // 不带 /v1（另一篇 Messages 文档写 .../v1/messages 是完整路径，照填会拼成
    // /v1/v1/messages，实测 404；/v1/messages 无 Key 401）。API 参考的 Messages
    // 示例用 Bearer，默认的 AUTH_TOKEN 即可；Key 以 doo-v1- 开头。排障表写明
    // BETA 标记会导致鉴权失败，须设 CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS。
    // 官方配置只设 base、用 haiku/sonnet/opus/fable 别名，API 参考说 model 可填
    // claude-opus-4-6 这类 Claude 原生 ID，故不写四档模型变量，按原生 ID 透传，
    // Claude Code 能照常识别 1M 窗口与思考档位；新原生 ID（如 Sonnet / Haiku
    // 5.5）能否被映射未经真 Key 实测。Tier 1 / Tier 2 账号用不了任何 Anthropic
    // 模型（docs.digitalocean.com/products/inference/details/limits）。
    // /v1/messages/count_tokens 实测 404。/v1/models 存在（无 Key 401），默认
    // 探测即可，不写 modelsUrl
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://inference.do-ai.run",
        ANTHROPIC_AUTH_TOKEN: "",
        CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS: "1",
      },
    },
    category: "cloud_provider",
    icon: "digitalocean",
    endpointCandidates: ["https://inference.do-ai.run"],
  },
  {
    name: "Hugging Face",
    websiteUrl: "https://huggingface.co/inference/models",
    apiKeyUrl:
      "https://huggingface.co/settings/tokens/new?ownUserPermissions=inference.serverless.write&tokenType=fineGrained",
    // Inference Providers 路由。官方 Claude Code 接入文档（huggingface.co/docs/
    // inference-providers/integrations/claude-code，2026-10-14 核对）：base 为
    // https://router.huggingface.co、不带 /v1（实测 GET /v1/messages 与
    // /v1/messages/count_tokens 均 405，路由存在）。Key 用勾选 "Make calls to
    // Inference Providers" 的 fine-grained token，示例发 Bearer，默认的
    // AUTH_TOKEN 即可；只发 x-api-key 能否通过未证实。模型 ID 一律钉上游
    // :baseten：文档说写明上游即确定路由，不写按吞吐挑选并自动切换，上游一变
    // prompt cache 前缀就丢；代价是没有故障转移，Baseten 挂了请求直接失败。
    // Baseten 同时提供这几款且窗口均为 1048576（公开 /v1/models 同日快照）。
    // 非 claude- 前缀 ID 默认按 200K 算：各档带 [1M]（表单预勾），另照官方
    // hf-claude 启动器写 CLAUDE_CODE_MAX_CONTEXT_TOKENS 兜底未勾 1M 的档位。/v1/models 公开可读（返回的 ID 不带上游
    // 后缀），默认探测即可，不写 modelsUrl
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://router.huggingface.co",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "zai-org/GLM-5.3:baseten[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "zai-org/GLM-5.3-Flash:baseten[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "zai-org/GLM-5.3:baseten[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "zai-org/GLM-5.3:baseten[1M]",
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "1048576",
      },
    },
    category: "aggregator",
    endpointCandidates: ["https://router.huggingface.co"],
    icon: "huggingface",
  },
  {
    name: "Microsoft Foundry",
    websiteUrl: "https://azure.microsoft.com/en-us/products/ai-foundry",
    // Key 和 Target URI 都在 Foundry 门户 Build → Models → 部署的 Details 标签页
    //（Azure 门户里是资源的 Keys and Endpoint），没有直达 Key 的页面
    apiKeyUrl: "https://ai.azure.com/",
    // 原名 Azure AI Foundry。官方部署文档（learn.microsoft.com/en-us/azure/
    // foundry/foundry-models/how-to/use-foundry-models-claude，2026-10-14 核对）：
    // base 为 https://<资源名>.services.ai.azure.com/anthropic、不带 /v1（实测
    // /anthropic/v1/messages 无 Key 401，少了 /v1 则 404），资源名由用户填。
    // REST 示例发 x-api-key，Anthropic 文档（platform.claude.com/docs/en/
    // build-with-claude/claude-in-microsoft-foundry）与官方 foundry-sdk 同，故
    // Key 填 ANTHROPIC_API_KEY；Hermes 有用户报 x-api-key 401、Bearer 可用
    //（hermes-agent #26970 / #29414），未经真 Key 实测。模型名填的是部署名，
    // 默认等于模型 ID，部署后不能改，自定义了名字须同步改；Mythos 系列只认
    // Entra，不收。走 ANTHROPIC_BASE_URL 时 Claude Code 当普通网关，而
    // cloud_provider 不会自动补 AUTO_MODE_SERVER=0，故手写。Foundry 不支持
    // Models API（文档同上），「获取模型列表」会失败，不写 modelsUrl。原生
    // CLAUDE_CODE_USE_FOUNDRY 模式没有 BASE_URL，走不了本地路由，不用。须付费
    // Azure 订阅并在 Marketplace 订阅 Claude，学生、试用、CSP 订阅不可用；开了
    // ZDR 的订阅调用 Covered Model 返回 400
    apiKeyField: "ANTHROPIC_API_KEY",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://${AZURE_RESOURCE_NAME}.services.ai.azure.com/anthropic",
        ANTHROPIC_API_KEY: "",
        ANTHROPIC_MODEL: "claude-opus-5-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-haiku-5-5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-5-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-5-5",
        CLAUDE_CODE_AUTO_MODE_SERVER: "0",
      },
    },
    category: "cloud_provider",
    templateValues: {
      AZURE_RESOURCE_NAME: {
        label: "Azure Resource Name",
        placeholder: "my-foundry-resource",
        editorValue: "",
      },
    },
    icon: "azure",
    iconColor: "#0078D4",
  },
  {
    name: "Fireworks AI",
    websiteUrl: "https://fireworks.ai",
    apiKeyUrl: "https://app.fireworks.ai/settings/users/api-keys",
    // 官方 Anthropic 兼容文档（docs.fireworks.ai/tools-sdks/
    // anthropic-compatibility，2026-10-14 核对）：base 为
    // https://api.fireworks.ai/inference、不带 /v1（实测 /inference/v1/messages
    // 未知模型 404 "Model not found"、/inference/messages 404 "Path not found"），
    // Bearer 鉴权；文档说 Key 放 ANTHROPIC_API_KEY 可能触发 "Not logged in"，
    // 故用默认的 AUTH_TOKEN。模型 ID 写全名 accounts/fireworks/models/<id>：
    // 官方 Claude Code 示例即全名，短 ID 并非每款都能解析（glm-5p2 短 ID 404、
    // 全名 401）。不用会漂移的 glm-latest / kimi-latest 别名，按官方 Open Models
    // 页（docs.fireworks.ai/nexus/open-models）写它们当前指向的固定 ID；分档照
    // 官方推荐模型页（docs.fireworks.ai/guides/recommended-models）：Kimi K3 顶
    // Opus、DeepSeek V4.1 Flash 顶 Haiku、GLM 5.3 做编程主力。GLM 5.3 只收文本，
    // 默认档贴图会失败。兼容层不支持 cache_control 与服务端工具，
    // count_tokens 实测 404，max_tokens 必填。仅美国节点 us.api.fireworks.ai
    // 标准档贵约 50%（docs.fireworks.ai/serverless/pricing），不放进候选。
    // /inference/v1/models 无 Key 401、路径存在，默认探测即可，不写 modelsUrl
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.fireworks.ai/inference",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "accounts/fireworks/models/glm-5p3[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL:
          "accounts/fireworks/models/deepseek-v4p1-flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "accounts/fireworks/models/glm-5p3[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "accounts/fireworks/models/kimi-k3[1M]",
      },
    },
    category: "cloud_provider",
    icon: "fireworks",
    endpointCandidates: ["https://api.fireworks.ai/inference"],
  },
  {
    name: "Mistral AI",
    websiteUrl: "https://mistral.ai",
    apiKeyUrl: "https://console.mistral.ai/api-keys",
    // 官方 OpenAPI（docs.mistral.ai/openapi.yaml，2026-10-14 核对）只列
    // /v1/chat/completions，鉴权为 Bearer。/v1/messages 与 /v1/responses 无 Key
    // 实测 401（不是 404，网关配了路由），但文档与 changelog 均未提，未经真 Key
    // 实测前只走 openai_chat 路由；base 不带 /v1，适配器自拼
    // /v1/chat/completions（同 Nebius）。模型取官方模型表（docs.mistral.ai/
    // models.md 同日），用固定版本 ID：主力 Mistral Medium 3.5、haiku 档
    // Mistral Small 4，均为 GA。Devstral 全系已下线（官方替代即这两款）；
    // Codestral 定位补全 / FIM，不做 Agent 主力。ChatCompletionRequest 不收
    // 额外字段且没有 stream_options，转换层给流式请求加的
    // stream_options.include_usage 会不会被 422 拒、开推理后 content 变成
    // thinking/text 分块数组能否解析，均未经真 Key 实测。/v1/models 无 Key 401，
    // 默认探测即可，不写 modelsUrl
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.mistral.ai",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "mistral-medium-3-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "mistral-small-2603",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "mistral-medium-3-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "mistral-medium-3-5",
        // 两款窗口官方只写「256k」（模型卡 docs.mistral.ai/models/
        // mistral-medium-3-5-26-04.md、mistral-small-4-0-26-03.md 同日），规则是
        // prompt + max_tokens ≤ 窗口；models.dev 记 262144，取较小的 256000 钉住
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "256000",
        CLAUDE_CODE_AUTO_COMPACT_WINDOW: "256000",
      },
    },
    category: "cn_official",
    apiFormat: "openai_chat",
    endpointCandidates: ["https://api.mistral.ai"],
    icon: "mistral",
    // 先隐藏：上面两条转换层风险（stream_options 可能 422、推理分块数组）
    // 真 Key 冒烟通过前不在列表里展示
    hidden: true,
  },
  {
    name: "Cloudflare AI Gateway",
    websiteUrl: "https://developers.cloudflare.com/ai-gateway/",
    // 网关设置页 dash.cloudflare.com/?to=/:account/ai/ai-gateway 也能建 token
    //（developers.cloudflare.com/ai-gateway/get-started，2026-10-14 核对）
    apiKeyUrl: "https://dash.cloudflare.com/profile/api-tokens",
    // 走厂商原生透传端点：base 为 https://gateway.ai.cloudflare.com/v1/<账户 ID>/
    // <网关 ID>/anthropic、不带 /v1（官方 Claude Code 接入页 developers.
    // cloudflare.com/ai-gateway/integrations/coding-agents/claude-code，
    // 2026-10-14 核对）。该页写法是 ANTHROPIC_API_KEY 填网关 token 再加
    // ANTHROPIC_CUSTOM_HEADERS 的 cf-aig-authorization 头，但本地路由不转发自定义
    // 头，靠它鉴权的卡进路由会 401；官方 Claude Desktop 接入页（…/coding-agents/
    // claude-desktop 同日）对同一端点写的是 Bearer 直接带网关 token（Key 是自己
    // 的 Anthropic Key 时才另加该头），故用默认的 AUTH_TOKEN 发 Bearer，只填一次
    // token、路由模式也能用。未经真 Key 实测：Unified Billing 页说请求带了厂商
    // Key 会原样转给厂商，Bearer 网关 token 会不会被当成 Anthropic Key 转发。
    // 网关 ID 默认 default（首个带鉴权的请求自动创建，get-started 同日），token
    // 须有 AI Gateway Run 权限。走 Unified Billing 预充值（充值收 5%，推理按厂商
    // 原价）或 BYOK 存自家 Key。模型用 Anthropic 原生连字符 ID（点号 ID 属于 REST
    // 统一 API，这里不用），三款窗口均 1M（developers.cloudflare.com/ai/models
    // 同日）。走 ANTHROPIC_BASE_URL 时 Claude Code 当普通网关，而 cloud_provider
    // 不会自动补 AUTO_MODE_SERVER=0，故手写（同 Microsoft Foundry）
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://gateway.ai.cloudflare.com/v1/${ACCOUNT_ID}/${GATEWAY_ID}/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "claude-sonnet-5-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-haiku-5-5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-5-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-5-5",
        CLAUDE_CODE_AUTO_MODE_SERVER: "0",
      },
    },
    category: "cloud_provider",
    templateValues: {
      ACCOUNT_ID: {
        label: "Account ID",
        placeholder: "32-character account ID",
        editorValue: "",
      },
      GATEWAY_ID: {
        label: "Gateway ID",
        placeholder: "default",
        defaultValue: "default",
        editorValue: "default",
      },
    },
    icon: "cloudflare",
  },
  {
    name: "GitHub Copilot",
    websiteUrl: "https://github.com/features/copilot",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.githubcopilot.com",
        ANTHROPIC_MODEL: "claude-sonnet-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-haiku-4.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-sonnet-5",
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
        ANTHROPIC_MODEL: "gpt-5.6-sol",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "gpt-5.6-luna",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "gpt-5.6-sol",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "gpt-5.6-sol",
        // Claude Code falls back to a 200K context window for unrecognized
        // non-Claude model ids. The ChatGPT Codex backend catalogs gpt-5.6-sol
        // at a 272K window (Codex budgets 95% of it), not the 1.05M API window;
        // it was 372K until mid-2026 (openai/codex#31860). Same cap as every
        // GPT window CC Switch fills (GPT_CONTEXT_WINDOW_CAP). Pin both knobs:
        // the compact window equals min(model window, value), and the routing
        // alias gets a 1M model window, so the compact window is what keeps the
        // trigger under the backend's limit.
        // Tweak these directly in the JSON editor; no form fields on purpose.
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "272000",
        CLAUDE_CODE_AUTO_COMPACT_WINDOW: "272000",
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
    name: "xAI (Grok)",
    websiteUrl: "https://x.ai/grok",
    settingsConfig: {
      env: {
        // The proxy enforces both this origin and the Responses wire format.
        ANTHROPIC_BASE_URL: "https://api.x.ai/v1",
        ANTHROPIC_MODEL: "grok-4.5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "grok-4.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "grok-4.5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "grok-4.5",
        // Claude Code falls back to 200K for non-Claude model ids; grok-4.5
        // has a 500K window (same as grok_config::DEFAULT_CONTEXT_WINDOW).
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "500000",
      },
    },
    category: "third_party",
    apiFormat: "openai_responses",
    providerType: "xai_oauth",
    requiresOAuth: true,
    icon: "xai",
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
        ANTHROPIC_MODEL: "moonshotai/kimi-k3[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "moonshotai/kimi-k3[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "moonshotai/kimi-k3[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "moonshotai/kimi-k3[1M]",
      },
    },
    category: "aggregator",
    apiFormat: "openai_chat",
    icon: "nvidia",
    iconColor: "#000000",
  },
  {
    name: "PIPELLM",
    websiteUrl: "https://code.pipellm.ai",
    apiKeyUrl: "https://code.pipellm.ai/login?ref=uvw650za",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://cc-api.pipellm.ai",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "claude-opus-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-haiku-4-5-20251001",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-5",
      },
    },
    category: "aggregator",
    icon: "pipellm",
  },
  {
    name: "Xiaomi MiMo",
    family: "xiaomi-mimo",
    planKey: "payg",
    websiteUrl: "https://platform.xiaomimimo.com",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/api-keys",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.xiaomimimo.com/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "mimo-v2.6-pro[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "mimo-v2.6-pro[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "mimo-v2.6-pro[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "mimo-v2.6-pro[1M]",
      },
    },
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
  },
  {
    name: "Xiaomi MiMo Token Plan (China)",
    family: "xiaomi-mimo",
    planKey: "tokenPlan",
    websiteUrl: "https://platform.xiaomimimo.com/#/token-plan",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/plan-manage",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://token-plan-cn.xiaomimimo.com/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "mimo-v2.6-pro[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "mimo-v2.6-pro[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "mimo-v2.6-pro[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "mimo-v2.6-pro[1M]",
      },
    },
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
  },
  {
    name: "AWS Bedrock (AKSK)",
    family: "aws-bedrock",
    planKey: "aksk",
    websiteUrl: "https://aws.amazon.com/bedrock/",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://bedrock-runtime.${AWS_REGION}.amazonaws.com",
        AWS_ACCESS_KEY_ID: "${AWS_ACCESS_KEY_ID}",
        AWS_SECRET_ACCESS_KEY: "${AWS_SECRET_ACCESS_KEY}",
        AWS_REGION: "${AWS_REGION}",
        ANTHROPIC_MODEL: "global.anthropic.claude-opus-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL:
          "global.anthropic.claude-haiku-4-5-20251001-v1:0",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "global.anthropic.claude-sonnet-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "global.anthropic.claude-opus-5",
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
    family: "aws-bedrock",
    planKey: "apiKey",
    websiteUrl: "https://aws.amazon.com/bedrock/",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL:
          "https://bedrock-runtime.${AWS_REGION}.amazonaws.com",
        // Claude Code 只从这个变量读 Bedrock API Key，顶层 apiKey 它不认
        AWS_BEARER_TOKEN_BEDROCK: "",
        AWS_REGION: "${AWS_REGION}",
        ANTHROPIC_MODEL: "global.anthropic.claude-opus-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL:
          "global.anthropic.claude-haiku-4-5-20251001-v1:0",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "global.anthropic.claude-sonnet-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "global.anthropic.claude-opus-5",
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
  {
    name: "JieKou AI",
    websiteUrl: "https://jiekou.ai/#model-library",
    apiKeyUrl: "https://jiekou.ai/settings/key-management",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.jiekou.ai/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "claude-fable-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-fable-5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-fable-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-fable-5",
      },
    },
    category: "aggregator",
    endpointCandidates: ["https://api.jiekou.ai/anthropic"],
    // 同 Novita：Anthropic 在 /anthropic、OpenAI 在 /openai/v1，根路径无
    // /models（实测 404），通用候选够不到，故覆写
    modelsUrl: "https://api.jiekou.ai/openai/v1/models",
    icon: "jiekou",
    iconColor: "#000000",
  },
  {
    name: "AICodeWith",
    websiteUrl: "https://aicodewith.ai",
    apiKeyUrl: "https://aicodewith.ai/login?tab=register",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.aicodewith.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    endpointCandidates: ["https://api.aicodewith.ai"],
    icon: "aicodewith",
    iconColor: "#3A3B40",
  },
  {
    name: "Command Code",
    websiteUrl: "https://commandcode.ai",
    apiKeyUrl: "https://commandcode.ai/settings/keys",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.commandcode.ai/provider",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "deepseek/deepseek-v4.1-flash[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "deepseek/deepseek-v4.1-flash[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "deepseek/deepseek-v4.1-flash[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "deepseek/deepseek-v4.1-flash[1M]",
      },
    },
    category: "third_party",
    apiFormat: "openai_chat",
    endpointCandidates: ["https://api.commandcode.ai/provider"],
    modelsUrl: "https://api.commandcode.ai/provider/v1/models",
    icon: "commandcode",
  },
  {
    name: "模力方舟",
    websiteUrl: "https://moark.com",
    apiKeyUrl: "https://moark.com/dashboard/tokens",
    settingsConfig: {
      env: {
        // 官方文档（CC Switch 快速配置）指定 Claude Code 走 Anthropic 原生入口
        ANTHROPIC_BASE_URL: "https://moark.com/anthropic",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "deepseek-v4-flash-0731[1M]",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "deepseek-v4-flash-0731[1M]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "deepseek-v4-flash-0731[1M]",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "deepseek-v4-flash-0731[1M]",
      },
    },
    category: "aggregator",
    endpointCandidates: ["https://moark.com/anthropic"],
    icon: "moark",
  },
  {
    name: "SoruxGPT",
    family: "soruxgpt",
    planKey: "payg",
    websiteUrl: "https://ai.soruxgpt.com",
    apiKeyUrl: "https://ai.soruxgpt.com/console/token",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://ai.soruxgpt.com",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: [
      "https://ai.soruxgpt.com",
      "https://api-cf.soruxgpt.com",
      "https://api-host.soruxgpt.com",
      "https://api-node6.soruxgpt.com",
      "https://api-node3.soruxgpt.com",
    ],
    category: "aggregator",
    icon: "soruxgpt",
  },
  {
    name: "SoruxGPT Coding Plan",
    family: "soruxgpt",
    planKey: "codingPlan",
    websiteUrl: "https://app.soruxgpt.com",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://app.soruxgpt.com/api/cc",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: [
      "https://app.soruxgpt.com/api/cc",
      "https://center.soruxgpt.com/api/cc",
      "https://u.soruxgpt.com/api/cc",
    ],
    category: "aggregator",
    icon: "soruxgpt",
  },
  {
    name: "IKunCode",
    websiteUrl: "https://api.ikuncode.ai",
    apiKeyUrl: "https://api.ikuncode.ai/console/token",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.ikuncode.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://api.ikuncode.ai", "https://api.ikuncode.cc"],
    category: "aggregator",
    icon: "ikuncode",
  },
  {
    name: "RunToken",
    websiteUrl: "https://www.runtoken.ai",
    apiKeyUrl: "https://www.runtoken.ai/console/token",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://www.runtoken.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: ["https://www.runtoken.ai", "https://fast.runtoken.ai"],
    category: "aggregator",
    icon: "runtoken",
  },
  {
    name: "BotCF",
    websiteUrl: "https://botcf.com",
    apiKeyUrl: "https://botcf.com/console/token",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://botcf.com",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    icon: "botcf",
  },
  {
    name: "YYLX",
    nameKey: "providerForm.presets.yylx",
    websiteUrl: "https://yylx.io",
    apiKeyUrl: "https://app.yylx.io/keys",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://app.yylx.io",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    endpointCandidates: [
      "https://app.yylx.io",
      "https://global.yylx.io",
      "https://cn.yylx.io",
    ],
    category: "aggregator",
    icon: "yylx",
  },
  {
    name: "TokenFlux",
    websiteUrl: "https://tokenflux.dev",
    apiKeyUrl: "https://tokenflux.dev/keys",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://tokenflux.dev",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    icon: "tokenflux",
  },
  {
    name: "NekoCode",
    websiteUrl: "https://nekoapi.ai",
    apiKeyUrl: "https://nekoapi.ai/token",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://nekoapi.ai",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    icon: "nekocode",
  },
  {
    name: "AIComing",
    websiteUrl: "https://aicoming.top",
    apiKeyUrl: "https://aicoming.top/console#keys",
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.aicoming.top",
        ANTHROPIC_AUTH_TOKEN: "",
      },
    },
    category: "aggregator",
    icon: "aicoming",
  },
];
