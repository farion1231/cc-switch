/**
 * Hermes Agent provider presets configuration
 * Hermes uses custom_providers array in config.yaml
 */
import type { ProviderCategory } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";
import type { PresetFamilyFields } from "./presetFamilies";

/**
 * Marker field and source values that `hermes_config.rs::get_providers`
 * injects onto each settings payload. Kept in sync with the Rust constants
 * `PROVIDER_SOURCE_FIELD` / `PROVIDER_SOURCE_CUSTOM_LIST` / `PROVIDER_SOURCE_DICT`.
 */
export const HERMES_PROVIDER_SOURCE_FIELD = "_cc_source";
export const HERMES_PROVIDER_SOURCE_CUSTOM_LIST = "custom_providers";
export const HERMES_PROVIDER_SOURCE_DICT = "providers_dict";

/**
 * True when the provider was sourced from Hermes' v12+ `providers:` dict —
 * CC Switch renders those read-only and routes edits to Hermes Web UI.
 */
export function isHermesReadOnlyProvider(settingsConfig: unknown): boolean {
  if (!settingsConfig || typeof settingsConfig !== "object") {
    return false;
  }
  const marker = (settingsConfig as Record<string, unknown>)[
    HERMES_PROVIDER_SOURCE_FIELD
  ];
  return marker === HERMES_PROVIDER_SOURCE_DICT;
}

/**
 * A model entry under a Hermes custom_provider.
 *
 * Serialized to YAML as a dict keyed by `id`:
 *
 * ```yaml
 * models:
 *   anthropic/claude-opus-5:
 *     context_length: 200000
 * ```
 *
 * Hermes' `_VALID_CUSTOM_PROVIDER_FIELDS` (hermes_cli/config.py) does not include
 * `max_tokens` at the per-model level — writing it produces an "unknown field"
 * warning on Hermes startup. Max tokens is a per-request parameter, not a
 * provider-level config.
 */
export interface HermesModel {
  /** Model ID — becomes the YAML key and the value written to top-level model.default. */
  id: string;
  /** Optional display label (UI only, not serialized to YAML). */
  name?: string;
  /** Override the auto-detected context window. */
  context_length?: number;
  /**
   * Declares that the model accepts image input. Hermes reads it per model
   * (`agent/image_routing.py`, `vision` is an alias); when absent it falls
   * back to its own catalog and treats unknown models as text-only.
   */
  supports_vision?: boolean;
}

/**
 * Top-level `model:` defaults suggested by a preset.
 *
 * Written to the YAML `model:` section when the user switches to this provider.
 * Per-model `context_length` lives on the individual `HermesModel` entries and
 * flows through `custom_providers[].models`, not this object.
 */
export interface HermesSuggestedDefaults {
  model: {
    /** Model ID for `model.default`. Typically equals `models[0].id`. */
    default: string;
    /** Value for `model.provider`. Omit to use the custom_provider name. */
    provider?: string;
  };
}

/** Hermes custom_provider protocol mode. Always written explicitly. */
export type HermesApiMode =
  | "chat_completions"
  | "anthropic_messages"
  | "codex_responses"
  | "bedrock_converse";

/** Default mode used when a provider has no stored value yet. */
export const HERMES_DEFAULT_API_MODE: HermesApiMode = "chat_completions";

/** Dropdown options for the API Mode selector. `labelKey` is looked up in i18n. */
export const hermesApiModes: Array<{
  value: HermesApiMode;
  labelKey: string;
}> = [
  { value: "chat_completions", labelKey: "hermes.form.apiModeChatCompletions" },
  {
    value: "anthropic_messages",
    labelKey: "hermes.form.apiModeAnthropicMessages",
  },
  { value: "codex_responses", labelKey: "hermes.form.apiModeCodexResponses" },
  {
    value: "bedrock_converse",
    labelKey: "hermes.form.apiModeBedrockConverse",
  },
];

export interface HermesProviderPreset extends PresetFamilyFields {
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: HermesProviderSettingsConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
  primePartner?: boolean; // 旧版的置顶合作伙伴标记；v7 起界面不再读取，新预设不写
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  templateValues?: Record<string, TemplateValueConfig>;
  theme?: PresetTheme;
  icon?: string;
  iconColor?: string;
  isCustomTemplate?: boolean;
  /** Optional top-level `model:` defaults written on switch. */
  suggestedDefaults?: HermesSuggestedDefaults;
}

export interface HermesProviderSettingsConfig {
  name: string;
  base_url?: string;
  api_key?: string;
  api_mode?: HermesApiMode;
  /** UI-side ordered list; serialized to YAML as a dict keyed by id. */
  models?: HermesModel[];
  /** Delay in seconds between consecutive requests to this provider. */
  rate_limit_delay?: number;
  [key: string]: unknown;
}

export const hermesProviderPresets: HermesProviderPreset[] = [
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
      name: "kimi",
      base_url: "https://api.moonshot.cn/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "kimi-k2.7-code", name: "Kimi K2.7 Code" },
        { id: "kimi-k3", name: "Kimi K3", context_length: 1048576 },
        {
          id: "kimi-k2.7-code-highspeed",
          name: "Kimi K2.7 Code HighSpeed",
          context_length: 262144,
        },
        { id: "kimi-k2.6", name: "Kimi K2.6", context_length: 262144 },
      ],
    },
    category: "cn_official",
    partnerPromotionKey: "kimi",
    icon: "kimi",
    iconColor: "#6366F1",
    suggestedDefaults: {
      model: { default: "kimi-k2.7-code", provider: "kimi" },
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
    settingsConfig: {
      name: "kimi",
      base_url: "https://api.moonshot.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "kimi-k2.7-code", name: "Kimi K2.7 Code" },
        { id: "kimi-k3", name: "Kimi K3", context_length: 1048576 },
        {
          id: "kimi-k2.7-code-highspeed",
          name: "Kimi K2.7 Code HighSpeed",
          context_length: 262144,
        },
        { id: "kimi-k2.6", name: "Kimi K2.6", context_length: 262144 },
      ],
    },
    category: "cn_official",
    partnerPromotionKey: "kimi",
    icon: "kimi",
    iconColor: "#6366F1",
    suggestedDefaults: {
      model: { default: "kimi-k2.7-code", provider: "kimi" },
    },
  },
  {
    name: "Kimi For Coding",
    family: "kimi",
    planKey: "coding",
    regionKey: "cn",
    primePartner: true,
    websiteUrl: "https://www.kimi.com/code/?aff=cc-switch",
    settingsConfig: {
      name: "kimi_coding",
      base_url: "https://api.kimi.com/coding/",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [{ id: "kimi-for-coding", name: "Kimi For Coding" }],
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
    suggestedDefaults: {
      model: { default: "kimi-for-coding", provider: "kimi_coding" },
    },
  },
  // 海外/Global 变体：kimi.ai/code + api.kimi.ai 端点，其余与国内版一致
  {
    name: "Kimi For Coding Global",
    family: "kimi",
    planKey: "coding",
    regionKey: "intl",
    websiteUrl: "https://www.kimi.ai/code?aff=cc-switch",
    settingsConfig: {
      name: "kimi_coding",
      base_url: "https://api.kimi.ai/coding/",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [{ id: "kimi-for-coding", name: "Kimi For Coding" }],
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
    suggestedDefaults: {
      model: { default: "kimi-for-coding", provider: "kimi_coding" },
    },
  },
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.ai",
    apiKeyUrl: "https://www.packyapi.ai/register?aff=cc-switch",
    settingsConfig: {
      name: "packycode",
      base_url: "https://www.packyapi.ai",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          context_length: 1000000,
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "packycode",
    icon: "packycode",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "packycode" },
    },
  },
  {
    name: "ZetaAPI",
    websiteUrl: "https://zetaapi.ai",
    apiKeyUrl: "https://zetaapi.ai/go/u117",
    settingsConfig: {
      name: "zetaapi",
      base_url: "https://api.zetaapi.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "gpt-5.6-sol", name: "GPT-5.6 Sol" }],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "zetaapi",
    icon: "zetaapi",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "zetaapi" },
    },
  },
  {
    name: "APINebula",
    websiteUrl: "https://apinebula.ai",
    apiKeyUrl: "https://apinebula.ai/VjM74M",
    settingsConfig: {
      name: "apinebula",
      base_url: "https://apinebula.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
        },
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apinebula",
    icon: "apinebula",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "apinebula" },
    },
  },
  {
    name: "AICodeMirror",
    websiteUrl: "https://www.aicodemirror.ai",
    apiKeyUrl: "https://www.aicodemirror.ai/register?invitecode=9915W3",
    settingsConfig: {
      name: "aicodemirror",
      base_url: "https://api.aicodemirror.ai/api/claudecode",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicodemirror",
    icon: "aicodemirror",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "aicodemirror" },
    },
  },
  {
    name: "FennoAI",
    websiteUrl: "https://api.fenno.ai",
    apiKeyUrl:
      "https://api.fenno.ai/register?redirect=/purchase?tab=subscription%26group=16&aff=P9MR3D3PLCNL",
    settingsConfig: {
      name: "fenno",
      base_url: "https://api.fenno.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "gpt-5.6-sol", name: "GPT-5.6 Sol" }],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "fenno",
    icon: "fenno",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "fenno" },
    },
  },
  {
    name: "RunAPI",
    websiteUrl: "https://runapi.host",
    apiKeyUrl: "https://runapi.host/register?aff=iOKB",
    settingsConfig: {
      name: "runapi",
      base_url: "https://runapi.host",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
        { id: "claude-haiku-4-5", name: "Claude Haiku 4.5" },
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          context_length: 1000000,
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
      model: { default: "claude-sonnet-5", provider: "runapi" },
    },
  },
  {
    name: "OpenModel",
    websiteUrl: "https://www.openmodel.ai",
    apiKeyUrl: "https://www.openmodel.ai?ref=cc-switch",
    settingsConfig: {
      name: "openmodel",
      base_url: "https://api.openmodel.ai",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: { default: "claude-opus-5-5", provider: "openmodel" },
    },
    isPartner: true,
    partnerPromotionKey: "openmodel",
    icon: "openmodel",
  },
  {
    name: "GrooRoute Pro",
    websiteUrl: "https://grooroute.pro",
    apiKeyUrl: "https://grooroute.pro/sign-up?aff=qqm9",
    settingsConfig: {
      name: "grooroute_pro",
      base_url: "https://grooroute.pro",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          context_length: 1000000,
        },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: { default: "claude-opus-5-5", provider: "grooroute_pro" },
    },
    isPartner: true,
    partnerPromotionKey: "grooroute",
    icon: "grooroute",
  },
  {
    name: "TokenFactory",
    nameKey: "providerForm.presets.tokenfactory",
    websiteUrl: "https://tokenfactory.cn",
    apiKeyUrl: "https://console.tokenfactory.cn/login",
    settingsConfig: {
      name: "tokenfactory",
      // 官方 Hermes 文档（docs.tokenfactory.cn/tools/hermes，2026-10-15 核对）
      base_url: "https://api.tokenfactory.cn/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "glm-5.3", name: "GLM-5.3", context_length: 1048576 },
        { id: "kimi-k3", name: "Kimi K3", context_length: 1048576 },
        {
          id: "deepseek-v4-pro-0813",
          name: "DeepSeek V4 Pro",
          context_length: 1048576,
        },
        {
          id: "deepseek-v4.1-flash",
          name: "DeepSeek V4.1 Flash",
          context_length: 1048576,
        },
        {
          id: "glm-5.3-flash",
          name: "GLM-5.3-Flash",
          context_length: 1048576,
        },
        { id: "qwen3.8-flash", name: "Qwen3.8 Flash", context_length: 1000000 },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: { default: "glm-5.3", provider: "tokenfactory" },
    },
    icon: "tokenfactory",
  },
  {
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    settingsConfig: {
      name: "shengsuanyun",
      base_url: "https://router.shengsuanyun.com/api/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "openai/gpt-5.6-sol", name: "GPT-5.6 Sol" },
        {
          id: "openai/gpt-6-astra",
          name: "GPT-6 Astra",
          context_length: 272000,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "shengsuanyun",
    icon: "shengsuanyun",
    suggestedDefaults: {
      model: { default: "openai/gpt-5.6-sol", provider: "shengsuanyun" },
    },
  },
  {
    name: "AIGoCode",
    websiteUrl: "https://aigocode.net",
    apiKeyUrl: "https://aigocode.net/invite/CC-SWITCH",
    settingsConfig: {
      name: "aigocode",
      base_url: "https://api.aigocode.dev",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aigocode",
    icon: "aigocode",
    iconColor: "#5B7FFF",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "aigocode" },
    },
  },
  {
    name: "Qiniu",
    nameKey: "providerForm.presets.qiniu",
    websiteUrl: "https://s.qiniu.com/nMvAvy",
    apiKeyUrl: "https://s.qiniu.com/nMvAvy",
    settingsConfig: {
      name: "qiniu",
      base_url: "https://api.qnaigc.com/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
        { id: "moonshotai/kimi-k3", name: "Kimi K3", context_length: 1048576 },
        { id: "z-ai/glm-5.3", name: "GLM-5.3", context_length: 1048576 },
        {
          id: "z-ai/glm-5.3-flash",
          name: "GLM-5.3-Flash",
          context_length: 1048576,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "qiniu",
    icon: "qiniu",
    suggestedDefaults: {
      model: { default: "gpt-6-astra", provider: "qiniu" },
    },
  },
  {
    name: "AICoding",
    websiteUrl: "https://aicoding.inc",
    apiKeyUrl: "https://aicoding.inc/i/CCSWITCH",
    settingsConfig: {
      name: "aicoding",
      base_url: "https://api.aicoding.inc",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicoding",
    icon: "aicoding",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "aicoding" },
    },
  },
  {
    name: "SubRouter",
    websiteUrl: "https://subrouter.ai",
    apiKeyUrl: "https://subrouter.ai/register?aff=l3ri",
    settingsConfig: {
      name: "subrouter",
      base_url: "https://subrouter.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          context_length: 272000,
        },
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "subrouter",
    icon: "subrouter",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "subrouter" },
    },
  },
  {
    // FluxA AgentMarket 以合作价转售的百度智能云 TokenPlan：产品页写明
    // "purchase it through AgentMarket, then use Baidu AI Cloud's endpoint and
    // API key directly"，端点取其所链的百度国际站 Token Plan Enterprise 文档
    // （2026-09-16 版）team 专属基址 —— 与国内个人版 qianfan.baidubce.com/
    // .../personal 是两套部署，勿合并。OpenAI 兼容基址走
    // chat_completions。阵容与窗口按 FluxA 产品页模型表
    // （glm-5.2 500k ≠ 国内版千帆平台 1M，国际 team 部署口径，勿按国内预设
    // "修正"）；标注 Coming soon 的 deepseek-v4-pro-0813 / glm-5.3 不收。
    // Kimi K2.6 是定稿赞助文案点名的模型，FluxA 产品页模型表与百度国际站
    // team 文档都没列它：id / 窗口取 FluxA baidu-ai-cloud 模型目录（categories
    // 只有 text）与国内 Token Plan 预设（262144）双重印证，非臆造
    name: "FluxA Token Plan",
    websiteUrl: "https://agentmarket.fluxapay.xyz/",
    apiKeyUrl: "https://agentmarket.fluxapay.xyz/marketplace/tokenplans",
    settingsConfig: {
      name: "fluxa_tokenplan",
      base_url: "https://api.baiduqianfan.ai/v2/tokenplan/team",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "deepseek-v4-pro", name: "DeepSeek V4 Pro" },
        { id: "deepseek-v4-flash-0731", name: "DeepSeek V4 Flash 0731" },
        { id: "deepseek-v4-flash", name: "DeepSeek V4 Flash" },
        { id: "deepseek-v3.2", name: "DeepSeek V3.2" },
        { id: "glm-5.2", name: "GLM-5.2" },
        { id: "glm-5.1", name: "GLM-5.1" },
        { id: "glm-5", name: "GLM-5" },
        { id: "kimi-k2.6", name: "Kimi K2.6" },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "fluxa",
    icon: "fluxa",
    suggestedDefaults: {
      model: { default: "deepseek-v4-pro", provider: "fluxa_tokenplan" },
    },
  },
  {
    name: "88API",
    websiteUrl: "https://88api.ai",
    apiKeyUrl: "https://88api.ai/sign-up?aff=HSGY",
    settingsConfig: {
      name: "88api",
      base_url: "https://api.88api.ai",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-haiku-4-5",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          context_length: 1000000,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "88api",
    icon: "88api",
    suggestedDefaults: {
      model: { default: "claude-opus-5-5", provider: "88api" },
    },
  },
  {
    name: "APIKEY.FUN",
    websiteUrl: "https://apikey.fan",
    apiKeyUrl: "https://apikey.fan/register?aff=CCSwitch",
    settingsConfig: {
      name: "apikeyfun",
      base_url: "https://api.apikey.fan",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          context_length: 1000000,
        },
        {
          id: "claude-haiku-4-5",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apikeyfun",
    icon: "apikeyfun",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "apikeyfun" },
    },
  },
  {
    name: "9527CODE",
    websiteUrl: "https://9527.codes",
    apiKeyUrl: "https://9527.codes/register?aff=e5zI",
    settingsConfig: {
      name: "9527code",
      base_url: "https://9527.codes",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          context_length: 1000000,
        },
        {
          id: "claude-haiku-4-5",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          context_length: 1000000,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "9527code",
    icon: "9527code",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "9527code" },
    },
  },
  {
    name: "Code0",
    websiteUrl: "https://code0.ai",
    apiKeyUrl: "https://code0.ai/agent/register/B2XHxGjGmRvqgznY",
    settingsConfig: {
      name: "code0",
      base_url: "https://code0.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "gpt-5.6-sol", name: "GPT-5.6 Sol" },
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
      ],
    },
    category: "aggregator",
    icon: "code0",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "code0" },
    },
  },
  {
    name: "TeamoRouter",
    websiteUrl: "https://teamorouter.cn",
    apiKeyUrl:
      "https://teamorouter.cn/?utm_source=cc_switch&utm_medium=referral&utm_campaign=ai_directory",
    settingsConfig: {
      name: "teamorouter",
      base_url: "https://api.teamorouter.cn/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "gpt-5.6-sol", name: "GPT-5.6 Sol" }],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "teamorouter",
    icon: "teamorouter",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "teamorouter" },
    },
  },
  {
    name: "PPIO",
    websiteUrl: "https://ppio.com",
    apiKeyUrl: "https://ppio.com/activity/ccswitch",
    settingsConfig: {
      name: "ppio",
      base_url: "https://api.ppio.com/openai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "deepseek/deepseek-v4-flash-0731",
          name: "Deepseek V4 Flash 0731",
          context_length: 1048576,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ppio",
    icon: "ppio",
    iconColor: "#2874FF",
    suggestedDefaults: {
      model: {
        default: "deepseek/deepseek-v4-flash-0731",
        provider: "ppio",
      },
    },
  },
  {
    name: "ClaudeCN",
    websiteUrl: "https://claudecn.top",
    apiKeyUrl: "https://claudecn.ai/register?aff=HEL9",
    settingsConfig: {
      name: "claudecn",
      base_url: "https://claudecn.top",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
        { id: "claude-haiku-4-5", name: "Claude Haiku 4.5" },
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          context_length: 1000000,
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
      model: { default: "claude-sonnet-5", provider: "claudecn" },
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
      name: "ark_agentplan",
      base_url: "https://ark.cn-beijing.volces.com/api/plan",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "ark-code-latest",
          name: "Ark Code Latest",
        },
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "volcengine_agentplan",
    icon: "huoshan",
    iconColor: "#3370FF",
    suggestedDefaults: {
      model: {
        default: "ark-code-latest",
        provider: "ark_agentplan",
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
      name: "ark_codingplan",
      base_url: "https://ark.cn-beijing.volces.com/api/coding",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "ark-code-latest",
          name: "Ark Code Latest",
        },
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "volcengine_codingplan",
    icon: "huoshan",
    iconColor: "#3370FF",
    suggestedDefaults: {
      model: {
        default: "ark-code-latest",
        provider: "ark_codingplan",
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
      name: "byteplus",
      base_url: "https://ark.ap-southeast.bytepluses.com/api/coding",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "ark-code-latest",
          name: "Ark Code Latest",
        },
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "byteplus",
    icon: "byteplus",
    iconColor: "#3370FF",
    suggestedDefaults: {
      model: {
        default: "ark-code-latest",
        provider: "byteplus",
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
      name: "doubao_seed",
      base_url: "https://ark.cn-beijing.volces.com/api/compatible",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "doubao-seed-2-1-pro-260628",
          name: "Doubao Seed 2.1 Pro",
        },
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "doubaoseed",
    icon: "doubao",
    iconColor: "#3370FF",
    suggestedDefaults: {
      model: {
        default: "doubao-seed-2-1-pro-260628",
        provider: "doubao_seed",
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
      name: "siliconflow",
      base_url: "https://api.siliconflow.cn/v1",
      api_key: "",
      api_mode: "chat_completions",
      // 国内站 M2.5 已于 2026-09-11 下线，与 Codex/OpenCode 同换 V4 Flash
      models: [
        {
          id: "deepseek-ai/DeepSeek-V4-Flash",
          name: "DeepSeek V4 Flash",
          context_length: 1048576,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "siliconflow",
    icon: "siliconflow",
    iconColor: "#6E29F6",
    suggestedDefaults: {
      model: {
        default: "deepseek-ai/DeepSeek-V4-Flash",
        provider: "siliconflow",
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
      name: "siliconflow_en",
      base_url: "https://api.siliconflow.com/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "MiniMaxAI/MiniMax-M3", name: "MiniMax M3" }],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "siliconflow",
    icon: "siliconflow",
    iconColor: "#000000",
    suggestedDefaults: {
      model: {
        default: "MiniMaxAI/MiniMax-M3",
        provider: "siliconflow_en",
      },
    },
  },
  {
    name: "A6API",
    websiteUrl: "https://www.a6api.com",
    apiKeyUrl: "https://a6api.com/register?aff=AqNr",
    settingsConfig: {
      name: "a6api",
      base_url: "https://api.a6api.com/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "gpt-5.6-sol", name: "GPT-5.6 Sol" },
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "a6api",
    icon: "a6api",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "a6api" },
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
      name: "compshare",
      base_url: "https://api.modelverse.cn/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
        { id: "kimi-k3", name: "Kimi K3", context_length: 1048576 },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ucloud",
    icon: "ucloud",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "gpt-6-astra", provider: "compshare" },
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
      name: "compshare_coding",
      base_url: "https://cp.compshare.cn/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "gpt-5.6-sol", name: "GPT-5.6 Sol" }],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ucloud",
    icon: "ucloud",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "compshare_coding" },
    },
  },
  {
    name: "CCSub",
    websiteUrl: "https://www.ccsub.net",
    apiKeyUrl: "https://www.ccsub.net/register?ref=Y6Z8DXEA",
    settingsConfig: {
      name: "ccsub",
      base_url: "https://www.ccsub.net/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "gpt-6-astra",
          name: "GPT-6 Astra",
          context_length: 272000,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ccsub",
    icon: "ccsub",
    suggestedDefaults: {
      model: { default: "gpt-6-astra", provider: "ccsub" },
    },
  },
  {
    name: "SSSAiCode",
    websiteUrl: "https://sssaicodeapi.com",
    apiKeyUrl: "https://sssaicodeapi.com/register?ref=DCP0SM",
    settingsConfig: {
      name: "sssaicode",
      base_url: "https://node-hk.sssaicodeapi.com/api",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "sssaicode",
    icon: "sssaicode",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "sssaicode" },
    },
  },
  {
    name: "SoleAPI",
    websiteUrl: "https://soleapi.com",
    apiKeyUrl: "https://soleapi.com/r/ccswitch",
    settingsConfig: {
      name: "soleapi",
      base_url: "https://soleapi.com",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          context_length: 1000000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "soleapi",
    icon: "soleapi",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "soleapi" },
    },
  },
  {
    name: "Micu",
    websiteUrl: "https://www.micuapi.ai",
    apiKeyUrl: "https://www.micuapi.ai/register?aff=aOYQ",
    settingsConfig: {
      name: "micu",
      base_url: "https://www.micuapi.ai",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "micu",
    icon: "micu",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "micu" },
    },
  },
  {
    name: "RightCode",
    websiteUrl: "https://www.rightapi.ai",
    apiKeyUrl: "https://www.rightapi.ai/register?aff=CCSWITCH",
    settingsConfig: {
      name: "rightcode",
      base_url: "https://www.rightapi.ai/claude",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "rightcode",
    icon: "rc",
    iconColor: "#E96B2C",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "rightcode" },
    },
  },
  {
    name: "Cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    settingsConfig: {
      name: "cubence",
      base_url: "https://api.cubence.com",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "cubence",
    icon: "cubence",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "cubence" },
    },
  },
  {
    name: "CrazyRouter",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    settingsConfig: {
      name: "crazyrouter",
      base_url: "https://cn.crazyrouter.com",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          context_length: 1000000,
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "crazyrouter",
    icon: "crazyrouter",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "crazyrouter" },
    },
  },
  {
    name: "DMXAPI",
    websiteUrl: "https://www.dmxapi.cn",
    apiKeyUrl: "https://www.dmxapi.cn",
    settingsConfig: {
      name: "dmxapi",
      base_url: "https://www.dmxapi.cn/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "gpt-5.6-sol", name: "GPT-5.6 Sol" }],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "dmxapi",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "dmxapi" },
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
      name: "sudocode",
      base_url: "https://api.sudocode.chat/v1",
      api_key: "",
      api_mode: "codex_responses",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
        },
        { id: "gpt-6-sol", name: "GPT-6 Sol", context_length: 272000 },
        { id: "gpt-6-luna", name: "GPT-6 Luna", context_length: 272000 },
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "sudocode",
    icon: "sudocode",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "sudocode" },
    },
  },
  {
    name: "SudoCode.us",
    family: "sudocode",
    websiteUrl: "https://sudocode.us",
    apiKeyUrl: "https://sudocode.us",
    settingsConfig: {
      name: "sudocode_us",
      base_url: "https://sudocode.us/v1",
      api_key: "",
      api_mode: "codex_responses",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
        },
        { id: "gpt-6-sol", name: "GPT-6 Sol", context_length: 272000 },
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
      ],
    },
    category: "third_party",
    isPartner: true,
    icon: "sudocode-us",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "sudocode_us" },
    },
  },
  {
    name: "XycAi",
    websiteUrl: "https://xycai.us",
    apiKeyUrl: "https://xycai.us/register?aff=Uhu9",
    settingsConfig: {
      name: "xycai",
      base_url: "https://apicdn.xycai.us/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "gpt-5.6-sol", name: "GPT-5.6 Sol" },
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "xycai",
    icon: "xycai",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "xycai" },
    },
  },
  {
    name: "Modelflare",
    websiteUrl: "https://modelflare.dev",
    apiKeyUrl:
      "https://modelflare.dev/sign-up?utm_medium=referral&utm_source=ccswitch",
    settingsConfig: {
      name: "modelflare",
      // Chat Completions + /v1（docs.modelflare.dev/guides/endpoints，
      // 2026-10-15 核对）；阵容同 OpenCode
      base_url: "https://modelflare.dev/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "gpt-5.6-sol", name: "GPT-5.6 Sol", context_length: 272000 },
        { id: "gpt-6-sol", name: "GPT-6 Sol", context_length: 272000 },
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "modelflare" },
    },
    isPartner: true,
    partnerPromotionKey: "modelflare",
    icon: "modelflare",
  },
  {
    name: "EveryAPI",
    websiteUrl: "https://everyapi.ai",
    apiKeyUrl:
      "https://app.everyapi.ai/signin?aff=FqlMa2eVVY&utm_source=ccswitch&utm_medium=invite",
    settingsConfig: {
      name: "everyapi",
      // GPT 系列只开放 Responses（api.everyapi.ai/api/pricing 的
      // supported_endpoint_types 为 openai-response，2026-10-17 核对）
      base_url: "https://api.everyapi.ai/v1",
      api_key: "",
      api_mode: "codex_responses",
      models: [
        { id: "gpt-5.6-sol", name: "GPT-5.6 Sol", context_length: 272000 },
        { id: "gpt-6-sol", name: "GPT-6 Sol", context_length: 272000 },
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "everyapi" },
    },
    isPartner: true,
    partnerPromotionKey: "everyapi",
    icon: "everyapi",
  },
  {
    name: "AllTokens",
    websiteUrl: "https://alltokens.cc",
    apiKeyUrl: "https://alltokens.cc/register",
    settingsConfig: {
      name: "alltokens",
      // Sub2API 网关：Key 绑定单个分组，须用 GPT 分组的 Key；GPT 账号池原生走
      // Responses（分组与模型见 alltokens.cc/api/v1/model-plaza，2026-10-17 核对）
      base_url: "https://alltokens.cc/v1",
      api_key: "",
      api_mode: "codex_responses",
      models: [
        { id: "gpt-5.6-sol", name: "GPT-5.6 Sol", context_length: 272000 },
        { id: "gpt-6-sol", name: "GPT-6 Sol", context_length: 272000 },
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "alltokens" },
    },
    isPartner: true,
    partnerPromotionKey: "alltokens",
    icon: "alltokens",
  },
  // ===== 非赞助商预设：应用内展示按显示名排序，此处文件顺序不影响展示 =====
  {
    name: "Tu-zi",
    nameKey: "providerForm.presets.tuzi",
    websiteUrl: "https://api.tu-zi.com",
    apiKeyUrl: "https://api.tu-zi.com/token",
    settingsConfig: {
      name: "tuzi",
      base_url: "https://api.tu-zi.com",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          context_length: 1000000,
        },
        {
          id: "claude-haiku-4-5",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
      ],
    },
    category: "aggregator",
    icon: "tuzi",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "tuzi" },
    },
  },
  {
    name: "Amux",
    websiteUrl: "https://amux.ai",
    apiKeyUrl: "https://amux.ai",
    settingsConfig: {
      name: "amux",
      base_url: "https://api.amux.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "gpt-5.6-sol", name: "GPT-5.6 Sol" },
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
      ],
    },
    category: "aggregator",
    icon: "amux",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "amux" },
    },
  },
  {
    name: "AtlasCloud",
    websiteUrl: "https://www.atlascloud.ai/console/coding-plan",
    apiKeyUrl: "https://www.atlascloud.ai/console/coding-plan",
    settingsConfig: {
      name: "atlascloud",
      base_url: "https://api.atlascloud.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "zai-org/glm-5.1",
          name: "GLM 5.1",
        },
      ],
    },
    category: "aggregator",
    icon: "atlascloud",
    suggestedDefaults: {
      model: { default: "zai-org/glm-5.1", provider: "atlascloud" },
    },
  },
  {
    name: "Soshow",
    websiteUrl: "https://aimarket.so-show.com",
    apiKeyUrl: "https://aimarket.so-show.com/workbench/access-key",
    settingsConfig: {
      name: "soshow",
      base_url: "https://maas.so-show.com",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
      ],
    },
    category: "aggregator",
    icon: "soshow",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "soshow" },
    },
  },
  {
    name: "OpenRouter",
    nameKey: "providerForm.presets.openrouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      name: "openrouter",
      base_url: "https://openrouter.ai/api/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "anthropic/claude-opus-5",
          name: "Claude Opus 5",
          context_length: 1000000,
        },
        {
          id: "anthropic/claude-sonnet-5",
          name: "Claude Sonnet 5",
          context_length: 1000000,
        },
        {
          id: "anthropic/claude-haiku-4.5",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
        {
          id: "openai/gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          context_length: 272000,
        },
        {
          id: "google/gemini-3.6-flash",
          name: "Gemini 3.6 Flash",
          context_length: 1000000,
        },
        {
          id: "anthropic/claude-opus-5.5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "anthropic/claude-fable-5.1",
          name: "Claude Fable 5.1",
          context_length: 1000000,
        },
        {
          id: "openai/gpt-6-astra",
          name: "GPT-6 Astra",
          context_length: 272000,
        },
        {
          id: "google/gemini-3.8-flash",
          name: "Gemini 3.8 Flash",
          context_length: 1048576,
        },
      ],
    },
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6366F1",
    suggestedDefaults: {
      model: { default: "anthropic/claude-opus-5", provider: "openrouter" },
    },
  },
  {
    name: "DeepSeek",
    nameKey: "providerForm.presets.deepseek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    settingsConfig: {
      name: "deepseek",
      base_url: "https://api.deepseek.com",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "deepseek-v4-pro",
          name: "DeepSeek V4 Pro",
          context_length: 1000000,
        },
        {
          id: "deepseek-flash",
          name: "DeepSeek V4.1 Flash",
          context_length: 1000000,
        },
      ],
    },
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#4D6BFE",
    suggestedDefaults: {
      model: { default: "deepseek-flash", provider: "deepseek" },
    },
  },
  {
    name: "Together AI",
    nameKey: "providerForm.presets.together",
    websiteUrl: "https://together.ai",
    apiKeyUrl: "https://api.together.ai/settings/api-keys",
    settingsConfig: {
      name: "together",
      base_url: "https://api.together.xyz/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "Qwen/Qwen3-Coder-480B-A35B-Instruct",
          name: "Qwen3 Coder 480B",
          context_length: 262144,
        },
        {
          id: "deepseek-ai/DeepSeek-V3.2",
          name: "DeepSeek V3.2",
          context_length: 64000,
        },
        {
          id: "meta-llama/Llama-4-Maverick-17B-128E-Instruct-FP8",
          name: "Llama 4 Maverick",
          context_length: 131072,
        },
      ],
    },
    category: "aggregator",
    icon: "together",
    iconColor: "#0F6FFF",
    suggestedDefaults: {
      model: {
        default: "Qwen/Qwen3-Coder-480B-A35B-Instruct",
        provider: "together",
      },
    },
  },
  {
    name: "Nous Research",
    websiteUrl: "https://nousresearch.com",
    apiKeyUrl: "https://portal.nousresearch.com/",
    settingsConfig: {
      name: "nous",
      base_url: "https://inference-api.nousresearch.com/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "Hermes-4-405B",
          name: "Hermes 4 405B",
          context_length: 131072,
        },
        {
          id: "Hermes-4-70B",
          name: "Hermes 4 70B",
          context_length: 131072,
        },
      ],
    },
    isOfficial: true,
    category: "official",
    icon: "hermes",
    iconColor: "#7C3AED",
    suggestedDefaults: {
      model: { default: "Hermes-4-405B", provider: "nous" },
    },
  },

  // 字段映射：env.ANTHROPIC_BASE_URL → base_url；env.ANTHROPIC_AUTH_TOKEN → api_key；
  // apiFormat "anthropic"(默认) → api_mode "anthropic_messages"；
  // apiFormat "openai_chat" → api_mode "chat_completions"；
  // ANTHROPIC_MODEL / DEFAULT_HAIKU / SONNET / OPUS_MODEL 去重后塞进 models[]。
  {
    name: "Zhipu GLM",
    family: "zhipu",
    regionKey: "cn",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    settingsConfig: {
      name: "zhipu_glm",
      base_url: "https://open.bigmodel.cn/api/coding/paas/v4",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "glm-5.3", name: "GLM-5.3" },
        { id: "glm-5.3-flash", name: "GLM-5.3-Flash", context_length: 1048576 },
      ],
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
    suggestedDefaults: {
      model: { default: "glm-5.3", provider: "zhipu_glm" },
    },
  },
  {
    name: "Zhipu GLM en",
    family: "zhipu",
    regionKey: "intl",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe?ic=8JVLJQFSKB",
    settingsConfig: {
      name: "zhipu_glm_en",
      base_url: "https://api.z.ai/api/coding/paas/v4",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "glm-5.3", name: "GLM-5.3" },
        { id: "glm-5.3-flash", name: "GLM-5.3-Flash", context_length: 1048576 },
      ],
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
    suggestedDefaults: {
      model: { default: "glm-5.3", provider: "zhipu_glm_en" },
    },
  },
  {
    // 腾讯云 Token Plan 个人版（1823/130060，2026-08-21 版）：通用 + Hy 两
    // 系列共用同一端点与 API Key，模型合并两系列；Auto 智能路由调用 ID 是
    // tc-code-latest。官方未发 Hermes 专属接入页，按工具无关的 /plan/v3
    // OpenAI Chat 端点收录（与市场线 1300/80643 的 chat_completions 模式
    // 一致）。kimi-k2.5 官方 2026-08-31 下线不收；minimax-m2.5 真 Key
    // 实测可用（2026-08-31）照实收
    name: "Tencent Token Plan",
    family: "tencent",
    planKey: "tokenPlan",
    regionKey: "cn",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan",
    settingsConfig: {
      name: "tencent_token_plan",
      base_url: "https://api.lkeap.cloud.tencent.com/plan/v3",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "tc-code-latest", name: "Auto" },
        { id: "deepseek-v4-flash-202605", name: "DeepSeek V4 Flash" },
        { id: "deepseek-v4-pro-202606", name: "DeepSeek V4 Pro" },
        { id: "minimax-m2.7", name: "MiniMax M2.7" },
        { id: "glm-5", name: "GLM-5" },
        { id: "glm-5.1", name: "GLM-5.1" },
        { id: "glm-5.2", name: "GLM-5.2" },
        { id: "hy3", name: "Hy3" },
        { id: "hy3-preview", name: "Hy3 Preview" },
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
    suggestedDefaults: {
      model: { default: "tc-code-latest", provider: "tencent_token_plan" },
    },
  },
  {
    // 国际站（新加坡地域）个人版（intl 1300/81315，2026-08-20 版）：Auto
    // 调用 ID 是 auto（≠国内 tc-code-latest），阵容不同（无 GLM-5/5.1/
    // Hy3）。端点用国际站文档钦定的 tencentcloudmaas.com 域；Key 按站独立
    name: "Tencent Token Plan (Intl)",
    family: "tencent",
    planKey: "tokenPlan",
    regionKey: "intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan",
    settingsConfig: {
      name: "tencent_token_plan_intl",
      base_url: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "auto", name: "Auto" },
        { id: "glm-5.2", name: "GLM-5.2" },
        { id: "kimi-k2.6", name: "Kimi K2.6" },
        { id: "deepseek-v4-pro-202606", name: "DeepSeek V4 Pro" },
        { id: "deepseek-v4-flash-202605", name: "DeepSeek V4 Flash" },
        { id: "minimax-m3", name: "MiniMax M3" },
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
    suggestedDefaults: {
      model: { default: "auto", provider: "tencent_token_plan_intl" },
    },
  },
  {
    // Token Plan 企业版专业套餐（1823/130659，2026-08-25 版，广州地域）：
    // kimi-k2.5 官方 2026-08-31 下线不收；minimax-m2.5 文档已除名但真 Key
    // 实测可用（2026-08-31），照实收录
    name: "Tencent Token Plan Enterprise Pro",
    family: "tencent",
    planKey: "enterprisePro",
    regionKey: "cn",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan-e",
    settingsConfig: {
      name: "tencent_token_plan_enterprise_pro",
      base_url: "https://tokenhub.tencentmaas.com/plan/v3",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "auto", name: "Auto" },
        { id: "glm-5.3", name: "GLM-5.3" },
        { id: "glm-5.2", name: "GLM-5.2" },
        { id: "glm-5", name: "GLM-5" },
        { id: "glm-5.1", name: "GLM-5.1" },
        { id: "glm-5-turbo", name: "GLM-5 Turbo" },
        { id: "kimi-k2.7-code", name: "Kimi K2.7 Code" },
        { id: "kimi-k2.7-code-highspeed", name: "Kimi K2.7 Code HighSpeed" },
        { id: "kimi-k2.6", name: "Kimi K2.6" },
        { id: "minimax-m2.7", name: "MiniMax M2.7" },
        { id: "minimax-m3", name: "MiniMax M3" },
        { id: "deepseek-v4-flash", name: "DeepSeek V4 Flash" },
        { id: "deepseek-v4-pro", name: "DeepSeek V4 Pro" },
        { id: "deepseek-v4-flash-0731", name: "DeepSeek V4 Flash 0731 GA" },
        { id: "deepseek-v4-pro-0813", name: "DeepSeek V4 Pro 0813 GA" },
        { id: "deepseek-v4-flash-202605", name: "DeepSeek V4 Flash Official" },
        { id: "deepseek-v4-pro-202606", name: "DeepSeek V4 Pro Official" },
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
    suggestedDefaults: {
      model: {
        default: "auto",
        provider: "tencent_token_plan_enterprise_pro",
      },
    },
  },
  {
    // 国际站企业版专业套餐（intl 1300/81489，2026-08-26 版，新加坡地域）：
    // 阵容为广州地域子集（无 GLM-5/5.1/5-Turbo、Kimi-K2.6、MiniMax-M2.7）
    name: "Tencent Token Plan Enterprise Pro (Intl)",
    family: "tencent",
    planKey: "enterprisePro",
    regionKey: "intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan-e",
    settingsConfig: {
      name: "tencent_token_plan_enterprise_pro_intl",
      base_url: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "auto", name: "Auto" },
        { id: "glm-5.3", name: "GLM-5.3" },
        { id: "glm-5.2", name: "GLM-5.2" },
        { id: "minimax-m3", name: "MiniMax M3" },
        { id: "kimi-k2.7-code", name: "Kimi K2.7 Code" },
        { id: "kimi-k2.7-code-highspeed", name: "Kimi K2.7 Code HighSpeed" },
        { id: "deepseek-v4-flash", name: "DeepSeek V4 Flash" },
        { id: "deepseek-v4-pro", name: "DeepSeek V4 Pro" },
        { id: "deepseek-v4-flash-0731", name: "DeepSeek V4 Flash 0731 GA" },
        { id: "deepseek-v4-pro-0813", name: "DeepSeek V4 Pro 0813 GA" },
        { id: "deepseek-v4-flash-202605", name: "DeepSeek V4 Flash Official" },
        { id: "deepseek-v4-pro-202606", name: "DeepSeek V4 Pro Official" },
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
    suggestedDefaults: {
      model: {
        default: "auto",
        provider: "tencent_token_plan_enterprise_pro_intl",
      },
    },
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
      name: "tencent_token_plan_enterprise_lite",
      base_url: "https://tokenhub.tencentmaas.com/plan/v3",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "auto", name: "Auto" }],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
    suggestedDefaults: {
      model: {
        default: "auto",
        provider: "tencent_token_plan_enterprise_lite",
      },
    },
  },
  {
    // 国际站企业版轻享套餐（intl 1300/81490）：新加坡地域（资源调度范围
    // Global），仅 Auto 模型
    name: "Tencent Token Plan Enterprise Lite (Intl)",
    family: "tencent",
    planKey: "enterpriseLite",
    regionKey: "intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan-e",
    settingsConfig: {
      name: "tencent_token_plan_enterprise_lite_intl",
      base_url: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "auto", name: "Auto" }],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#0052D9",
    suggestedDefaults: {
      model: {
        default: "auto",
        provider: "tencent_token_plan_enterprise_lite_intl",
      },
    },
  },
  {
    // 千帆 Token Plan 个人版（2026-07-13 起替代 Coding Plan 发售）：官方
    // Hermes 接入页确认 /v2/tokenplan/personal、默认 deepseek-v4-pro（其
    // api_mode 写 "openai_messages"，本仓 OpenAI Chat 端点惯例统一映射为
    // chat_completions）；阵容=Token Plan 主文档 2026-08-14 版六模型
    name: "Baidu Qianfan Token Plan",
    websiteUrl: "https://cloud.baidu.com/product/codingplan.html",
    apiKeyUrl: "https://console.bce.baidu.com/qianfan/resource/token-plan",
    settingsConfig: {
      name: "qianfan_tokenplan",
      base_url: "https://qianfan.baidubce.com/v2/tokenplan/personal",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "deepseek-v4-pro", name: "DeepSeek V4 Pro" },
        { id: "deepseek-v4-flash", name: "DeepSeek V4 Flash" },
        { id: "deepseek-v4-flash-0731", name: "DeepSeek V4 Flash 0731" },
        { id: "glm-5.2", name: "GLM-5.2" },
        { id: "glm-5.1", name: "GLM-5.1" },
        { id: "kimi-k2.6", name: "Kimi K2.6" },
      ],
    },
    category: "cn_official",
    icon: "baidu",
    iconColor: "#2932E1",
    suggestedDefaults: {
      model: { default: "deepseek-v4-pro", provider: "qianfan_tokenplan" },
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
      name: "qianwenai",
      base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "qwen3.8-max", name: "Qwen3.8 Max", context_length: 983616 },
        { id: "qwen3.8-flash", name: "Qwen3.8 Flash", context_length: 983616 },
      ],
    },
    category: "cn_official",
    icon: "qianwenai",
    iconColor: "#624AFF",
    suggestedDefaults: {
      model: { default: "qwen3.8-max", provider: "qianwenai" },
    },
  },
  {
    name: "千问AI平台 Coding Plan",
    family: "qianwen",
    planKey: "codingPlan",
    websiteUrl: "https://bailian.console.aliyun.com",
    settingsConfig: {
      name: "qianwenai_coding_plan",
      base_url: "https://coding.dashscope.aliyuncs.com/apps/anthropic",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "qwen3-coder-plus", name: "Qwen3 Coder Plus" },
        { id: "qwen3-max", name: "Qwen3 Max" },
      ],
    },
    category: "cn_official",
    icon: "qianwenai",
    iconColor: "#624AFF",
    suggestedDefaults: {
      model: {
        default: "qwen3-coder-plus",
        provider: "qianwenai_coding_plan",
      },
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
      name: "qianwenai_token_plan",
      base_url:
        "https://token-plan.cn-beijing.maas.aliyuncs.com/apps/anthropic",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "qwen3.8-max", name: "Qwen3.8 Max", context_length: 983616 },
        { id: "qwen3.8-flash", name: "Qwen3.8 Flash", context_length: 983616 },
      ],
    },
    category: "cn_official",
    icon: "qianwenai",
    iconColor: "#624AFF",
    suggestedDefaults: {
      model: { default: "qwen3.8-max", provider: "qianwenai_token_plan" },
    },
  },
  // ===== QwenCloud（DashScope 国际站）=====
  // 与上面国内条目是两套独立站点：域名、控制台、密钥互不通用。
  // QwenCloud 与 Token Plan 都用 anthropic_messages，地址不带 /v1（与官方
  // hermes 文档一致，这点和 OpenCode / OpenClaw 不同，勿互相照搬）。
  {
    name: "QwenCloud",
    family: "qwencloud",
    planKey: "payg",
    websiteUrl: "https://home.qwencloud.com/?utm_content=g_20000002974",
    apiKeyUrl: "https://home.qwencloud.com/api-keys?utm_content=g_20000002975",
    settingsConfig: {
      name: "qwencloud",
      base_url: "https://dashscope-intl.aliyuncs.com/apps/anthropic",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "qwen3.8-max", name: "Qwen3.8 Max", context_length: 983616 },
        { id: "qwen3.8-flash", name: "Qwen3.8 Flash", context_length: 983616 },
        { id: "qwen3.7-max", name: "Qwen3.7 Max", context_length: 1000000 },
      ],
    },
    category: "cn_official",
    icon: "qwencloud",
    iconColor: "#6336E7",
    suggestedDefaults: {
      model: { default: "qwen3.8-max", provider: "qwencloud" },
    },
  },
  {
    name: "QwenCloud For Coding",
    family: "qwencloud",
    planKey: "coding",
    websiteUrl: "https://www.qwencloud.com",
    apiKeyUrl: "https://home.qwencloud.com/api-keys",
    settingsConfig: {
      name: "qwencloud_coding",
      base_url: "https://coding-intl.dashscope.aliyuncs.com/apps/anthropic",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "qwen3.7-plus", name: "Qwen3.7 Plus", context_length: 1000000 },
        { id: "qwen3.6-plus", name: "Qwen3.6 Plus", context_length: 1000000 },
        {
          id: "qwen3-coder-plus",
          name: "Qwen3 Coder Plus",
          context_length: 131072,
        },
      ],
    },
    category: "cn_official",
    icon: "qwencloud",
    iconColor: "#6336E7",
    suggestedDefaults: {
      model: { default: "qwen3.7-plus", provider: "qwencloud_coding" },
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
      name: "qwencloud_token_plan",
      base_url:
        "https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "qwen3.8-max", name: "Qwen3.8 Max", context_length: 983616 },
        { id: "qwen3.8-flash", name: "Qwen3.8 Flash", context_length: 983616 },
        { id: "qwen3.7-max", name: "Qwen3.7 Max", context_length: 1000000 },
        { id: "qwen3.7-plus", name: "Qwen3.7 Plus", context_length: 1000000 },
        { id: "qwen3.6-plus", name: "Qwen3.6 Plus", context_length: 1000000 },
        { id: "qwen3.6-flash", name: "Qwen3.6 Flash", context_length: 1000000 },
      ],
    },
    category: "cn_official",
    icon: "qwencloud",
    iconColor: "#6336E7",
    suggestedDefaults: {
      model: { default: "qwen3.8-max", provider: "qwencloud_token_plan" },
    },
  },
  {
    name: "StepFun",
    websiteUrl: "https://platform.stepfun.ai",
    apiKeyUrl: "https://platform.stepfun.ai/interface-key",
    settingsConfig: {
      name: "stepfun",
      base_url: "https://api.stepfun.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "step-3.5-flash", name: "Step 3.5 Flash" },
        {
          id: "step-3.7-flash",
          name: "Step 3.7 Flash",
          context_length: 256000,
        },
        {
          id: "step-5-preview",
          name: "Step 5 Preview",
          context_length: 1000000,
        },
      ],
    },
    category: "cn_official",
    icon: "stepfun",
    iconColor: "#005AFF",
    suggestedDefaults: {
      model: { default: "step-3.5-flash", provider: "stepfun" },
    },
  },
  {
    name: "ModelScope",
    websiteUrl: "https://modelscope.cn",
    settingsConfig: {
      name: "modelscope",
      base_url: "https://api-inference.modelscope.cn/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "ZhipuAI/GLM-5.2", name: "ZhipuAI / GLM-5.2" }],
    },
    category: "aggregator",
    icon: "modelscope",
    iconColor: "#624AFF",
    suggestedDefaults: {
      model: { default: "ZhipuAI/GLM-5.2", provider: "modelscope" },
    },
  },
  {
    name: "StreamLake",
    family: "streamlake",
    regionKey: "cn",
    websiteUrl: "https://www.streamlake.com",
    apiKeyUrl: "https://console.streamlake.com/console/wanqing/api-key",
    settingsConfig: {
      name: "streamlake",
      base_url:
        "https://wanqing.streamlakeapi.com/api/gateway/v1/endpoints/kat-coder-pro-v2.5/claude-code-proxy",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [{ id: "kat-coder-pro-v2.5", name: "KAT-Coder Pro V2.5" }],
    },
    category: "cn_official",
    icon: "streamlake",
    suggestedDefaults: {
      model: { default: "kat-coder-pro-v2.5", provider: "streamlake" },
    },
  },
  {
    name: "StreamLake en",
    family: "streamlake",
    regionKey: "intl",
    websiteUrl: "https://console.streamlake.ai",
    apiKeyUrl: "https://console.streamlake.ai/console/api-key",
    settingsConfig: {
      name: "streamlake_en",
      base_url:
        "https://vanchin.streamlake.ai/api/gateway/v1/endpoints/kat-coder-pro-v2.5/claude-code-proxy",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [{ id: "kat-coder-pro-v2.5", name: "KAT-Coder Pro V2.5" }],
    },
    category: "cn_official",
    icon: "streamlake",
    suggestedDefaults: {
      model: { default: "kat-coder-pro-v2.5", provider: "streamlake_en" },
    },
  },
  {
    name: "Longcat",
    websiteUrl: "https://longcat.chat/platform",
    apiKeyUrl: "https://longcat.chat/platform/api_keys",
    settingsConfig: {
      name: "longcat",
      base_url: "https://api.longcat.chat/openai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "LongCat-2.0", name: "LongCat 2.0" }],
    },
    category: "cn_official",
    icon: "longcat",
    iconColor: "#29E154",
    suggestedDefaults: {
      model: { default: "LongCat-2.0", provider: "longcat" },
    },
  },
  {
    name: "MiniMax",
    family: "minimax",
    regionKey: "cn",
    websiteUrl: "https://platform.minimax.cn",
    apiKeyUrl: "https://platform.minimax.cn/subscribe/token-plan",
    settingsConfig: {
      name: "minimax",
      base_url: "https://api.minimax.cn/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "MiniMax-M3", name: "MiniMax M3" }],
    },
    category: "cn_official",
    theme: { backgroundColor: "#f64551", textColor: "#FFFFFF" },
    icon: "minimax",
    iconColor: "#FF6B6B",
    suggestedDefaults: {
      model: { default: "MiniMax-M3", provider: "minimax" },
    },
  },
  {
    name: "MiniMax en",
    family: "minimax",
    regionKey: "intl",
    websiteUrl: "https://platform.minimax.io",
    apiKeyUrl: "https://platform.minimax.io/subscribe/coding-plan",
    settingsConfig: {
      name: "minimax_en",
      base_url: "https://api.minimax.io/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "MiniMax-M3", name: "MiniMax M3" }],
    },
    category: "cn_official",
    theme: { backgroundColor: "#f64551", textColor: "#FFFFFF" },
    icon: "minimax",
    iconColor: "#FF6B6B",
    suggestedDefaults: {
      model: { default: "MiniMax-M3", provider: "minimax_en" },
    },
  },
  {
    name: "BaiLing",
    websiteUrl: "https://developer.ant-ling.com/zh-CN/docs/",
    apiKeyUrl: "https://chat.ant-ling.com/open",
    settingsConfig: {
      name: "bailing",
      base_url: "https://api.ant-ling.com/anthropic",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [{ id: "Ling-2.6-1T", name: "Ling 2.6 1T" }],
    },
    category: "cn_official",
    suggestedDefaults: {
      model: { default: "Ling-2.6-1T", provider: "bailing" },
    },
    icon: "bailing",
  },
  {
    name: "AiHubMix",
    websiteUrl: "https://aihubmix.com",
    apiKeyUrl: "https://aihubmix.com",
    settingsConfig: {
      name: "aihubmix",
      base_url: "https://aihubmix.com/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "gpt-5.6-sol", name: "GPT-5.6 Sol" },
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
      ],
    },
    category: "aggregator",
    icon: "aihubmix",
    iconColor: "#006FFB",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "aihubmix" },
    },
  },
  {
    name: "CherryIN",
    websiteUrl: "https://open.cherryin.ai",
    apiKeyUrl: "https://open.cherryin.ai/console/token",
    settingsConfig: {
      name: "cherryin",
      base_url: "https://open.cherryin.net",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "anthropic/claude-opus-5", name: "Claude Opus 5" },
        { id: "anthropic/claude-sonnet-5", name: "Claude Sonnet 5" },
        {
          id: "anthropic/claude-fable-5.1",
          name: "Claude Fable 5.1",
          context_length: 1000000,
        },
      ],
    },
    category: "aggregator",
    icon: "cherryin",
    suggestedDefaults: {
      model: { default: "anthropic/claude-opus-5", provider: "cherryin" },
    },
  },
  {
    name: "E-FlowCode",
    websiteUrl: "https://e-flowcode.cc",
    apiKeyUrl: "https://e-flowcode.cc",
    settingsConfig: {
      name: "eflowcode",
      base_url: "https://e-flowcode.cc",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          context_length: 1000000,
        },
      ],
    },
    category: "third_party",
    icon: "eflowcode",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "eflowcode" },
    },
  },
  {
    name: "TheRouter",
    websiteUrl: "https://therouter.ai",
    apiKeyUrl: "https://dashboard.therouter.ai",
    settingsConfig: {
      name: "therouter",
      base_url: "https://api.therouter.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "openai/gpt-5.6-sol", name: "GPT-5.6 Sol" },
        { id: "openai/gpt-5.4-mini", name: "GPT-5.4 mini" },
        { id: "openai/gpt-5.4-nano", name: "GPT-5.4 nano" },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: {
        default: "openai/gpt-5.6-sol",
        provider: "therouter",
      },
    },
    icon: "therouter",
  },
  {
    name: "Baseten",
    websiteUrl: "https://www.baseten.co",
    apiKeyUrl: "https://app.baseten.co/settings/api_keys",
    settingsConfig: {
      name: "baseten",
      // OpenAI Chat 端点 /v1/chat/completions（docs.baseten.co/development/
      // model-apis/overview，2026-10-14 核对）；窗口取同页模型表
      base_url: "https://inference.baseten.co/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "zai-org/GLM-5.3", name: "GLM-5.3", context_length: 1048576 },
        { id: "moonshotai/Kimi-K3", name: "Kimi K3", context_length: 1048576 },
        {
          id: "zai-org/GLM-5.3-Flash",
          name: "GLM-5.3 Flash",
          context_length: 1048576,
        },
        {
          id: "deepseek-ai/DeepSeek-V4.1-Flash",
          name: "DeepSeek V4.1 Flash",
          context_length: 1048576,
        },
      ],
    },
    category: "aggregator",
    icon: "baseten",
    suggestedDefaults: {
      model: { default: "zai-org/GLM-5.3", provider: "baseten" },
    },
  },
  {
    name: "Novita AI",
    websiteUrl: "https://novita.ai",
    apiKeyUrl: "https://novita.ai",
    settingsConfig: {
      name: "novita",
      base_url: "https://api.novita.ai/v3/openai",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "zai-org/glm-5.1", name: "Zai-Org / GLM-5.1" },
        { id: "zai-org/glm-5.3", name: "GLM-5.3", context_length: 1048576 },
        {
          id: "zai-org/glm-5.3-flash",
          name: "GLM-5.3-Flash",
          context_length: 1048576,
        },
        { id: "moonshotai/kimi-k3", name: "Kimi K3", context_length: 1048576 },
      ],
    },
    category: "aggregator",
    icon: "novita",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "zai-org/glm-5.1", provider: "novita" },
    },
  },
  {
    name: "Nvidia",
    websiteUrl: "https://build.nvidia.com",
    apiKeyUrl: "https://build.nvidia.com/settings/api-keys",
    settingsConfig: {
      name: "nvidia",
      base_url: "https://integrate.api.nvidia.com",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "moonshotai/kimi-k3",
          name: "Moonshot Kimi K3",
          context_length: 1048576,
        },
        { id: "z-ai/glm-5.3", name: "GLM-5.3", context_length: 1048576 },
        {
          id: "z-ai/glm-5.3-flash",
          name: "GLM-5.3-Flash",
          context_length: 1048576,
        },
      ],
    },
    category: "aggregator",
    icon: "nvidia",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "moonshotai/kimi-k3", provider: "nvidia" },
    },
  },
  {
    name: "Ollama Cloud",
    websiteUrl: "https://ollama.com",
    apiKeyUrl: "https://ollama.com/settings/keys",
    settingsConfig: {
      // Hermes 自带内置 provider ollama-cloud（读 OLLAMA_API_KEY），用下划线
      // 名避免与之同名，确保走这里的 custom_providers 与 api_key
      name: "ollama_cloud",
      // 云端 OpenAI 兼容 base（docs.ollama.com/api/openai-compatibility，
      // 2026-10-14 核对）；窗口取 https://ollama.com/api/show
      base_url: "https://ollama.com/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "glm-5.3", name: "GLM-5.3", context_length: 1048576 },
        {
          id: "glm-5.3-flash",
          name: "GLM-5.3-Flash",
          context_length: 1048576,
        },
        { id: "kimi-k3", name: "Kimi K3", context_length: 1048576 },
        {
          id: "deepseek-v4-pro:0813",
          name: "DeepSeek V4 Pro",
          context_length: 1048576,
        },
        {
          id: "deepseek-v4.1-flash",
          name: "DeepSeek V4.1 Flash",
          context_length: 1048576,
        },
        { id: "minimax-m3", name: "MiniMax M3", context_length: 512000 },
      ],
    },
    category: "cloud_provider",
    icon: "ollama",
    suggestedDefaults: {
      model: { default: "glm-5.3", provider: "ollama_cloud" },
    },
  },
  {
    name: "Ollama",
    websiteUrl: "https://ollama.com",
    settingsConfig: {
      // Hermes 里 ollama / local 都是指向 custom 的别名（hermes_cli/auth.py），
      // 照 ollama_cloud 先例换名避开
      name: "ollama-local",
      // Hermes 官方本机写法：base http://localhost:11434/v1、Key 留空
      //（hermes-agent.nousresearch.com/docs/integrations/providers，2026-10-14
      // 核对）。Hermes 启动时拒绝小于 64000 的窗口：这里填模型原生值，服务端
      // 实际窗口也要至少 64k（OLLAMA_CONTEXT_LENGTH 或 model.ollama_num_ctx）。
      // 模型换成自己已下载的
      base_url: "http://localhost:11434/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "qwen3.5", name: "Qwen3.5", context_length: 262144 },
        { id: "gpt-oss:20b", name: "GPT-OSS 20B", context_length: 131072 },
      ],
    },
    category: "local",
    icon: "ollama",
    suggestedDefaults: {
      model: { default: "qwen3.5", provider: "ollama-local" },
    },
  },
  {
    name: "LM Studio",
    websiteUrl: "https://lmstudio.ai",
    settingsConfig: {
      // Hermes 内置 provider lmstudio，且 lm-studio / lm_studio 都是它的别名
      //（hermes_cli/auth.py），照 ollama-local 先例换名避开
      name: "lmstudio-local",
      // base http://localhost:1234/v1、默认不鉴权，Key 留空（开启鉴权后填 API
      // Token；hermes-agent.nousresearch.com/docs/integrations/providers，
      // 2026-10-14 核对）。Hermes 启动时拒绝小于 64000 的窗口：这里填模型原生
      // 值，LM Studio 加载模型时的 context length 也要至少 64K（UI 设置或
      // lms load --context-length），加载窗口更小时同步调小。模型换成自己已
      // 下载的，或用「获取模型」从 /v1/models 拉取
      base_url: "http://localhost:1234/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "openai/gpt-oss-20b",
          name: "GPT-OSS 20B",
          context_length: 131072,
        },
      ],
    },
    category: "local",
    icon: "lmstudio",
    suggestedDefaults: {
      model: { default: "openai/gpt-oss-20b", provider: "lmstudio-local" },
    },
  },
  {
    name: "llama.cpp",
    websiteUrl: "https://github.com/ggml-org/llama.cpp",
    settingsConfig: {
      // Hermes 里 llamacpp / llama.cpp / llama-cpp 都是指向 custom 的别名
      //（hermes_cli/auth.py、providers.py），照 lmstudio-local 先例换名避开
      name: "llamacpp-local",
      // 本机 llama-server 的 OpenAI Chat base（github.com/ggml-org/llama.cpp/
      // blob/master/tools/server/README.md、hermes-agent.nousresearch.com/docs/
      // integrations/providers，2026-10-14 核对）。默认端口 b11521（PR #30159）
      // 起为 9931，v0.6.0 及更早、Homebrew 与 Docker 仍是 8080；只监听 IPv4 的
      // 127.0.0.1。不带 --api-key 时不校验，Key 留空（带了就填那个值）。Hermes
      // 启动时拒绝小于 64000 的窗口：这里填模型原生值，llama-server 的 -c 也要
      // 至少 64K（--fit 可能自动调小），实际窗口更小时同步调小。默认模型取
      // ggml-org 官方 GGUF 的 Qwen3-Coder 30B A3B，换成自己已下载的，或用
      // 「获取模型」从 /v1/models 拉取
      base_url: "http://127.0.0.1:9931/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "ggml-org/Qwen3-Coder-30B-A3B-Instruct-Q8_0-GGUF",
          name: "Qwen3-Coder 30B A3B",
          context_length: 262144,
        },
      ],
    },
    category: "local",
    icon: "llamacpp",
    suggestedDefaults: {
      model: {
        default: "ggml-org/Qwen3-Coder-30B-A3B-Instruct-Q8_0-GGUF",
        provider: "llamacpp-local",
      },
    },
  },
  {
    name: "vLLM",
    websiteUrl: "https://vllm.ai",
    settingsConfig: {
      // Hermes 里 vllm / local 都是指向 custom 的别名（hermes_cli/providers.py），
      // 照 llamacpp-local 先例换名避开
      name: "vllm-local",
      // 本机 vllm serve 的 OpenAI Chat base（docs.vllm.ai/en/latest/serving/
      // online_serving/openai_compatible_server、hermes-agent.nousresearch.com/
      // docs/integrations/providers，2026-10-14 核对）。默认端口 8000，不带
      // --host 时监听全部 IPv4 网卡，建议加 --host 127.0.0.1；工具调用要启动时加
      // --enable-auto-tool-choice --tool-call-parser qwen3_coder。不带 --api-key
      // 时不校验，Key 留空（Hermes 会发占位值 no-key-required；带了就填那个值）。
      // Hermes 启动时拒绝小于 64000 的窗口：这里填模型原生值，--max-model-len
      // 也要至少 64K，实际窗口更小时同步调小。默认模型取 vLLM Codex 接入页的
      // 示例 Qwen/Qwen3.6-27B，须与 /v1/models 的 id 一致，换成自己已下载的，
      // 或用「获取模型」从 /v1/models 拉取
      base_url: "http://localhost:8000/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "Qwen/Qwen3.6-27B",
          name: "Qwen3.6 27B",
          context_length: 262144,
        },
      ],
    },
    category: "local",
    icon: "vllm",
    suggestedDefaults: {
      model: { default: "Qwen/Qwen3.6-27B", provider: "vllm-local" },
    },
  },
  {
    name: "Meta Model API",
    websiteUrl: "https://dev.meta.ai",
    apiKeyUrl: "https://dev.meta.ai",
    settingsConfig: {
      name: "meta",
      // OpenAI Chat 端点 /v1/chat/completions（dev.meta.ai/docs/coding-agents.md，
      // 2026-10-14 核对），Bearer 鉴权；窗口取 dev.meta.ai/docs/models.md
      base_url: "https://api.meta.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "muse-wonder-1.0",
          name: "Muse Wonder 1.0",
          context_length: 1048576,
        },
        {
          id: "muse-spark-1.3",
          name: "Muse Spark 1.3",
          context_length: 1048576,
        },
      ],
    },
    category: "cn_official",
    icon: "meta",
    suggestedDefaults: {
      model: { default: "muse-wonder-1.0", provider: "meta" },
    },
  },
  {
    name: "DeepInfra",
    websiteUrl: "https://deepinfra.com",
    apiKeyUrl: "https://deepinfra.com/dash/api_keys",
    settingsConfig: {
      name: "deepinfra",
      // 官方 Hermes 接入文档给旧版 Hermes 的自定义端点即 /v1/openai Chat
      //（docs.deepinfra.com/integrations/hermes-agent.md，2026-10-14 核对）。
      // Hermes 已内置 deepinfra，同名写法照 openrouter / deepseek 先例；窗口取
      // https://api.deepinfra.com/models/<id> 同日元数据
      base_url: "https://api.deepinfra.com/v1/openai",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "deepseek-ai/DeepSeek-V4-Flash-0731",
          name: "DeepSeek V4 Flash 0731",
          context_length: 1048576,
        },
        {
          id: "deepseek-ai/DeepSeek-V4-Pro-0813",
          name: "DeepSeek V4 Pro 0813",
          context_length: 1048576,
        },
        { id: "moonshotai/Kimi-K3", name: "Kimi K3", context_length: 1048576 },
        { id: "zai-org/GLM-5.3", name: "GLM-5.3", context_length: 1048576 },
        { id: "zai-org/GLM-5.2", name: "GLM-5.2", context_length: 1048576 },
      ],
    },
    category: "aggregator",
    icon: "deepinfra",
    suggestedDefaults: {
      model: {
        default: "deepseek-ai/DeepSeek-V4-Flash-0731",
        provider: "deepinfra",
      },
    },
  },
  {
    name: "Vercel AI Gateway",
    websiteUrl: "https://vercel.com/ai-gateway",
    apiKeyUrl:
      "https://vercel.com/d?to=%2F%5Bteam%5D%2F%7E%2Fai-gateway%2Fapi-keys",
    settingsConfig: {
      // 与 Vercel CLI 写的自定义供应商同名（vercel.com/docs/ai-gateway/
      // coding-agents/hermes，2026-10-14 核对）；Hermes 内置的 ai-gateway 默认
      // 走裸 /v1，文档推荐改成 /coding-agent/v1。窗口取 GET
      // https://ai-gateway.vercel.sh/v1/models 同日公开元数据
      name: "vercel-ai-gateway",
      base_url: "https://ai-gateway.vercel.sh/coding-agent/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "anthropic/claude-opus-5.5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "anthropic/claude-sonnet-5.5",
          name: "Claude Sonnet 5.5",
          context_length: 1000000,
        },
        {
          id: "anthropic/claude-haiku-5.5",
          name: "Claude Haiku 5.5",
          context_length: 1000000,
        },
        {
          id: "openai/gpt-6-astra",
          name: "GPT-6 Astra",
          context_length: 272000,
        },
        {
          id: "openai/gpt-6.1-sol",
          name: "GPT-6.1 Sol",
          context_length: 272000,
        },
        {
          id: "google/gemini-3.8-flash",
          name: "Gemini 3.8 Flash",
          context_length: 1000000,
        },
      ],
    },
    category: "aggregator",
    icon: "vercel",
    suggestedDefaults: {
      model: {
        default: "anthropic/claude-sonnet-5.5",
        provider: "vercel-ai-gateway",
      },
    },
  },
  {
    name: "Nebius Token Factory",
    websiteUrl: "https://nebius.com/token-factory",
    apiKeyUrl: "https://tokenfactory.nebius.com/project/api-keys",
    settingsConfig: {
      name: "nebius",
      // OpenAI Chat 端点 base 为 /v1（docs.tokenfactory.nebius.com/switch.md，
      // 2026-10-14 核对），Bearer 鉴权。窗口取官方公开目录
      // tokenfactory.nebius.com/api/public/models_info 同日快照
      base_url: "https://api.tokenfactory.nebius.com/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "zai-org/GLM-5.3", name: "GLM-5.3", context_length: 1024000 },
        {
          id: "zai-org/GLM-5.3-Flash",
          name: "GLM-5.3-Flash",
          context_length: 1024000,
        },
        { id: "moonshotai/Kimi-K3", name: "Kimi K3", context_length: 1024000 },
        {
          id: "moonshotai/Kimi-K2.7-Code",
          name: "Kimi K2.7 Code",
          context_length: 262144,
        },
        {
          id: "deepseek-ai/DeepSeek-V4-Pro-0813",
          name: "DeepSeek V4 Pro 0813",
          context_length: 979000,
        },
      ],
    },
    category: "aggregator",
    icon: "nebius",
    suggestedDefaults: {
      model: { default: "zai-org/GLM-5.3", provider: "nebius" },
    },
  },
  {
    name: "Groq",
    websiteUrl: "https://groq.com",
    apiKeyUrl: "https://console.groq.com/keys",
    settingsConfig: {
      name: "groq",
      // OpenAI Chat 端点 base 为 /openai/v1（console.groq.com/docs/openai，
      // 2026-10-14 核对），Bearer 鉴权。只收自助可用的 Production 模型 gpt-oss
      // 两款，窗口取 console.groq.com/docs/models 同日值
      base_url: "https://api.groq.com/openai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "openai/gpt-oss-120b",
          name: "GPT-OSS 120B",
          context_length: 131072,
        },
        {
          id: "openai/gpt-oss-20b",
          name: "GPT-OSS 20B",
          context_length: 131072,
        },
      ],
    },
    category: "aggregator",
    icon: "groq",
    suggestedDefaults: {
      model: { default: "openai/gpt-oss-120b", provider: "groq" },
    },
  },
  {
    name: "SambaNova",
    websiteUrl: "https://sambanova.ai",
    apiKeyUrl: "https://cloud.sambanova.ai/apis",
    settingsConfig: {
      name: "sambanova",
      // OpenAI Chat 端点 base 为 /v1（docs.sambanova.ai/docs/en/integrations/
      // opencode，2026-10-14 核对），Bearer 鉴权，窗口取公开 /v1/models 同日值。
      // 不选 anthropic_messages（OpenAPI 写明 v1 的 thinking enabled/adaptive
      // 返回 400），也不选 codex_responses（Responses 只收 function 工具）
      base_url: "https://api.sambanova.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "MiniMax-M3", name: "MiniMax M3", context_length: 1048576 },
        { id: "gpt-oss-120b", name: "gpt-oss-120b", context_length: 131072 },
      ],
    },
    category: "cloud_provider",
    icon: "sambanova",
    suggestedDefaults: {
      model: { default: "MiniMax-M3", provider: "sambanova" },
    },
  },
  {
    name: "DigitalOcean",
    websiteUrl: "https://www.digitalocean.com/products/ai-platform",
    apiKeyUrl: "https://cloud.digitalocean.com/model-studio/manage-keys",
    settingsConfig: {
      name: "digitalocean",
      // OpenAI Chat 端点 base 为 /v1（docs.digitalocean.com/products/inference/
      // how-to/use-with-coding-agents，2026-10-14 核对），Bearer 鉴权；窗口取
      // 平台模型表（docs.digitalocean.com/products/inference/details/models
      // 同日）。四款都支持 Chat 与工具调用；Sonnet 5.5 须 Tier 3+ 账号，默认
      // 用所有等级可用的 GLM-5.3
      base_url: "https://inference.do-ai.run/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "glm-5.3", name: "GLM-5.3", context_length: 1048576 },
        { id: "kimi-k3", name: "Kimi K3", context_length: 1048576 },
        {
          id: "deepseek-v4-pro-0813",
          name: "DeepSeek V4 Pro 0813",
          context_length: 1048576,
        },
        {
          id: "anthropic-claude-sonnet-5.5",
          name: "Claude Sonnet 5.5",
          context_length: 1000000,
        },
      ],
    },
    category: "cloud_provider",
    icon: "digitalocean",
    suggestedDefaults: {
      model: { default: "glm-5.3", provider: "digitalocean" },
    },
  },
  {
    name: "Hugging Face",
    websiteUrl: "https://huggingface.co/inference/models",
    apiKeyUrl:
      "https://huggingface.co/settings/tokens/new?ownUserPermissions=inference.serverless.write&tokenType=fineGrained",
    settingsConfig: {
      // Hermes 内置 provider huggingface（别名 hf，hermes-agent.nousresearch.com/
      // docs/integrations/providers，2026-10-14 核对），与 custom_providers 同名
      // 时谁优先文档没写，照 ollama_cloud 先例换个名字避开
      name: "huggingface-router",
      // OpenAI Chat 端点 base 为 https://router.huggingface.co/v1
      //（huggingface.co/docs/inference-providers/integrations/hermes-agent 同日），
      // Bearer 鉴权；Hermes 文档说可用 :provider_name 后缀强制上游，这里钉
      // :baseten（同 Claude Code 预设），窗口取公开 /v1/models 对 Baseten 的
      // 同日声明
      base_url: "https://router.huggingface.co/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "zai-org/GLM-5.3:baseten",
          name: "GLM-5.3 (Baseten)",
          context_length: 1048576,
        },
        {
          id: "zai-org/GLM-5.3-Flash:baseten",
          name: "GLM-5.3 Flash (Baseten)",
          context_length: 1048576,
        },
        {
          id: "moonshotai/Kimi-K3:baseten",
          name: "Kimi K3 (Baseten)",
          context_length: 1048576,
        },
        {
          id: "deepseek-ai/DeepSeek-V4-Pro-0813:baseten",
          name: "DeepSeek V4 Pro 0813 (Baseten)",
          context_length: 1048576,
        },
      ],
    },
    category: "aggregator",
    icon: "huggingface",
    suggestedDefaults: {
      model: {
        default: "zai-org/GLM-5.3:baseten",
        provider: "huggingface-router",
      },
    },
  },
  {
    name: "Fireworks AI",
    websiteUrl: "https://fireworks.ai",
    apiKeyUrl: "https://app.fireworks.ai/settings/users/api-keys",
    settingsConfig: {
      // Hermes 内置 provider fireworks（别名 fireworks-ai / fw，hermes-agent.
      // nousresearch.com/docs/integrations/providers，2026-10-14 核对），与
      // custom_providers 同名时谁优先文档没写，照 huggingface-router 先例换个
      // 名字避开
      name: "fireworks-inference",
      // OpenAI Chat 端点 base 为 https://api.fireworks.ai/inference/v1
      //（docs.fireworks.ai/nexus/harnesses 同日），Bearer 鉴权，模型写全名；
      // 窗口取各模型页与定价页（同日），MiniMax M3 只开 512K
      base_url: "https://api.fireworks.ai/inference/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "accounts/fireworks/models/glm-5p3",
          name: "GLM 5.3",
          context_length: 1048576,
        },
        {
          id: "accounts/fireworks/models/kimi-k3",
          name: "Kimi K3",
          context_length: 1048576,
        },
        {
          id: "accounts/fireworks/models/deepseek-v4p1-flash",
          name: "DeepSeek V4.1 Flash",
          context_length: 1000000,
        },
        {
          id: "accounts/fireworks/models/glm-5p3-flash",
          name: "GLM 5.3 Flash",
          context_length: 1048576,
        },
        {
          id: "accounts/fireworks/models/minimax-m3",
          name: "MiniMax M3",
          context_length: 512000,
        },
      ],
    },
    category: "cloud_provider",
    icon: "fireworks",
    suggestedDefaults: {
      model: {
        default: "accounts/fireworks/models/glm-5p3",
        provider: "fireworks-inference",
      },
    },
  },
  {
    name: "Mistral AI",
    websiteUrl: "https://mistral.ai",
    apiKeyUrl: "https://console.mistral.ai/api-keys",
    settingsConfig: {
      // Hermes 内置的 Mistral 只有语音与转写，没有 LLM provider，名字不冲突
      name: "mistral",
      // OpenAI Chat 端点 base 为 https://api.mistral.ai/v1（docs.mistral.ai/
      // openapi.yaml，2026-10-14 核对），Bearer 鉴权。模型用官方模型表
      //（docs.mistral.ai/models.md 同日）的固定版本 ID，窗口官方写 256k
      base_url: "https://api.mistral.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "mistral-medium-3-5",
          name: "Mistral Medium 3.5",
          context_length: 256000,
        },
        {
          id: "mistral-small-2603",
          name: "Mistral Small 4",
          context_length: 256000,
        },
      ],
    },
    category: "cn_official",
    icon: "mistral",
    suggestedDefaults: {
      model: { default: "mistral-medium-3-5", provider: "mistral" },
    },
  },
  {
    name: "Cloudflare AI Gateway",
    websiteUrl: "https://developers.cloudflare.com/ai-gateway/",
    apiKeyUrl: "https://dash.cloudflare.com/profile/api-tokens",
    settingsConfig: {
      // Hermes 没有内置的 Cloudflare provider，名字不冲突
      name: "cloudflare",
      // REST 统一 API 的 OpenAI Chat 端点（developers.cloudflare.com/ai-gateway/
      // usage/rest-api，2026-10-14 核对），标准 Bearer 鉴权，token 须带 Workers
      // AI Read；账户 ID 用字面占位符，忘了改会 401。模型与窗口同 OpenClaw 预设
      //（developers.cloudflare.com/ai/models 同日），Claude 只有 Messages 格式不放
      base_url:
        "https://api.cloudflare.com/client/v4/accounts/YOUR_ACCOUNT_ID/ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "openai/gpt-6.1-sol",
          name: "GPT-6.1 Sol",
          context_length: 272000,
        },
        {
          id: "openai/gpt-6-luna",
          name: "GPT-6 Luna",
          context_length: 272000,
        },
        {
          id: "moonshotai/kimi-k3",
          name: "Kimi K3",
          context_length: 1048576,
        },
      ],
    },
    category: "cloud_provider",
    icon: "cloudflare",
    suggestedDefaults: {
      model: { default: "openai/gpt-6.1-sol", provider: "cloudflare" },
    },
  },
  {
    name: "PIPELLM",
    websiteUrl: "https://code.pipellm.ai",
    apiKeyUrl: "https://code.pipellm.ai/login?ref=uvw650za",
    settingsConfig: {
      name: "pipellm",
      base_url: "https://cc-api.pipellm.ai",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-5", name: "Claude Opus 5" },
        { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
        },
      ],
    },
    category: "aggregator",
    icon: "pipellm",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "pipellm" },
    },
  },
  {
    name: "Xiaomi MiMo",
    family: "xiaomi-mimo",
    planKey: "payg",
    websiteUrl: "https://platform.xiaomimimo.com",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/api-keys",
    settingsConfig: {
      name: "xiaomi_mimo",
      base_url: "https://api.xiaomimimo.com/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "mimo-v2.6-pro", name: "MiMo V2.6 Pro", context_length: 1048576 },
        {
          id: "mimo-v2.6-flash",
          name: "MiMo V2.6 Flash",
          context_length: 1048576,
        },
        {
          id: "mimo-v2.6-pro-ultraspeed",
          name: "MiMo V2.6 Pro UltraSpeed",
          context_length: 1048576,
        },
        { id: "mimo-v2.5-pro", name: "MiMo v2.5 Pro" },
      ],
    },
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "mimo-v2.6-pro", provider: "xiaomi_mimo" },
    },
  },
  {
    name: "Xiaomi MiMo Token Plan (China)",
    family: "xiaomi-mimo",
    planKey: "tokenPlan",
    websiteUrl: "https://platform.xiaomimimo.com/#/token-plan",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/plan-manage",
    settingsConfig: {
      name: "xiaomi_mimo_token_plan",
      base_url: "https://token-plan-cn.xiaomimimo.com/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "mimo-v2.6-pro", name: "MiMo V2.6 Pro", context_length: 1048576 },
        {
          id: "mimo-v2.6-flash",
          name: "MiMo V2.6 Flash",
          context_length: 1048576,
        },
        { id: "mimo-v2.5-pro", name: "MiMo v2.5 Pro" },
        { id: "mimo-v2.5", name: "MiMo v2.5" },
      ],
    },
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "mimo-v2.6-pro", provider: "xiaomi_mimo_token_plan" },
    },
  },
  {
    name: "JieKou AI",
    websiteUrl: "https://jiekou.ai/#model-library",
    apiKeyUrl: "https://jiekou.ai/settings/key-management",
    settingsConfig: {
      name: "jiekou",
      base_url: "https://api.jiekou.ai/openai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "claude-fable-5",
          name: "Claude Fable 5",
          context_length: 1000000,
        },
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-fable-5-1",
          name: "Claude Fable 5.1",
          context_length: 1000000,
        },
      ],
    },
    category: "aggregator",
    icon: "jiekou",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-fable-5", provider: "jiekou" },
    },
  },
  {
    name: "AICodeWith",
    websiteUrl: "https://aicodewith.ai",
    apiKeyUrl: "https://aicodewith.ai/login?tab=register",
    settingsConfig: {
      name: "aicodewith",
      base_url: "https://api.aicodewith.ai/v1",
      api_key: "",
      api_mode: "codex_responses",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
        },
        { id: "gpt-6-sol", name: "GPT-6 Sol", context_length: 272000 },
        { id: "gpt-6-astra", name: "GPT-6 Astra", context_length: 272000 },
      ],
    },
    category: "aggregator",
    icon: "aicodewith",
    iconColor: "#3A3B40",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "aicodewith" },
    },
  },
  {
    name: "SoruxGPT",
    websiteUrl: "https://ai.soruxgpt.com",
    apiKeyUrl: "https://ai.soruxgpt.com/console/token",
    settingsConfig: {
      name: "soruxgpt",
      base_url: "https://ai.soruxgpt.com",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: { default: "claude-opus-5-5", provider: "soruxgpt" },
    },
    icon: "soruxgpt",
  },
  {
    name: "IKunCode",
    websiteUrl: "https://api.ikuncode.ai",
    apiKeyUrl: "https://api.ikuncode.ai/console/token",
    settingsConfig: {
      name: "ikuncode",
      base_url: "https://api.ikuncode.ai",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: { default: "claude-opus-5-5", provider: "ikuncode" },
    },
    icon: "ikuncode",
  },
  {
    name: "RunToken",
    websiteUrl: "https://www.runtoken.ai",
    apiKeyUrl: "https://www.runtoken.ai/console/token",
    settingsConfig: {
      name: "runtoken",
      base_url: "https://www.runtoken.ai",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: { default: "claude-opus-5-5", provider: "runtoken" },
    },
    icon: "runtoken",
  },
  {
    name: "BotCF",
    websiteUrl: "https://botcf.com",
    apiKeyUrl: "https://botcf.com/console/token",
    settingsConfig: {
      name: "botcf",
      base_url: "https://botcf.com",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: { default: "claude-opus-5-5", provider: "botcf" },
    },
    icon: "botcf",
  },
  {
    name: "YYLX",
    nameKey: "providerForm.presets.yylx",
    websiteUrl: "https://yylx.io",
    apiKeyUrl: "https://app.yylx.io/keys",
    settingsConfig: {
      name: "yylx",
      base_url: "https://app.yylx.io",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          context_length: 1000000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: { default: "claude-opus-5-5", provider: "yylx" },
    },
    icon: "yylx",
  },
  {
    name: "TokenFlux",
    websiteUrl: "https://tokenflux.dev",
    apiKeyUrl: "https://tokenflux.dev/keys",
    settingsConfig: {
      name: "tokenflux",
      base_url: "https://tokenflux.dev",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-haiku-4-5",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: { default: "claude-opus-5-5", provider: "tokenflux" },
    },
    icon: "tokenflux",
  },
  {
    name: "NekoCode",
    websiteUrl: "https://nekoapi.ai",
    apiKeyUrl: "https://nekoapi.ai/token",
    settingsConfig: {
      name: "nekocode",
      base_url: "https://nekoapi.ai",
      api_key: "",
      api_mode: "anthropic_messages",
      extra_headers: {
        "User-Agent": "claude-cli/2.1.161 (external, cli)",
      },
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: { default: "claude-opus-5-5", provider: "nekocode" },
    },
    icon: "nekocode",
  },
  {
    name: "AIComing",
    websiteUrl: "https://aicoming.top",
    apiKeyUrl: "https://aicoming.top/console#keys",
    settingsConfig: {
      name: "aicoming",
      base_url: "https://api.aicoming.top",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "claude-opus-5-5",
          name: "Claude Opus 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-sonnet-5-5",
          name: "Claude Sonnet 5.5",
          context_length: 1000000,
        },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: { default: "claude-opus-5-5", provider: "aicoming" },
    },
    icon: "aicoming",
  },
];
