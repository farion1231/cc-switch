/**
 * Hermes Agent provider presets configuration
 * Hermes uses custom_providers array in config.yaml
 */
import type { ProviderCategory } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

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

export interface HermesProviderPreset {
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: HermesProviderSettingsConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
  primePartner?: boolean; // 置顶合作伙伴（顶级）：徽章显示为心形
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
  // ===== 赞助商预设：文件顺序 = 应用内展示顺序，与 README 赞助商表对齐 =====
  {
    name: "Kimi",
    primePartner: true,
    websiteUrl: "https://platform.kimi.com?aff=cc-switch",
    settingsConfig: {
      name: "kimi",
      base_url: "https://api.moonshot.cn/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "kimi-k2.7-code", name: "Kimi K2.7 Code" },
        { id: "kimi-k3", name: "Kimi K3", context_length: 1048576 },
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
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    settingsConfig: {
      name: "shengsuanyun",
      base_url: "https://router.shengsuanyun.com/api/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "openai/gpt-5.6-sol", name: "GPT-5.6 Sol" }],
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
    websiteUrl: "https://aigocode.app",
    apiKeyUrl: "https://aigocode.app/invite/CC-SWITCH",
    settingsConfig: {
      name: "aigocode",
      base_url: "https://api.aigocode.app",
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
      models: [{ id: "gpt-5.6-sol", name: "GPT-5.6 Sol" }],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "qiniu",
    icon: "qiniu",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "qiniu" },
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
          context_length: 400000,
        },
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
    name: "APIKEY.FUN",
    websiteUrl: "https://apikey.fun",
    apiKeyUrl: "https://apikey.fun/register?aff=CCSwitch",
    settingsConfig: {
      name: "apikeyfun",
      base_url: "https://api.apikey.fun",
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
      models: [{ id: "gpt-5.6-sol", name: "GPT-5.6 Sol" }],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "code0",
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
    name: "DouBaoSeed",
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
    websiteUrl: "https://siliconflow.cn",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/YflgU2Ve",
    settingsConfig: {
      name: "siliconflow",
      base_url: "https://api.siliconflow.cn/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "Pro/MiniMaxAI/MiniMax-M2.5",
          name: "Pro / MiniMax M2.5",
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
        default: "Pro/MiniMaxAI/MiniMax-M2.5",
        provider: "siliconflow",
      },
    },
  },
  {
    name: "SiliconFlow en",
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
      models: [{ id: "gpt-5.6-sol", name: "GPT-5.6 Sol" }],
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
    isPartner: true,
    partnerPromotionKey: "atlascloud",
    icon: "atlascloud",
    suggestedDefaults: {
      model: { default: "zai-org/glm-5.1", provider: "atlascloud" },
    },
  },
  {
    name: "Compshare",
    nameKey: "providerForm.presets.ucloud",
    websiteUrl: "https://www.compshare.cn",
    apiKeyUrl:
      "https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch",
    settingsConfig: {
      name: "compshare",
      base_url: "https://api.modelverse.cn/v1",
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
      model: { default: "gpt-5.6-sol", provider: "compshare" },
    },
  },
  {
    name: "Compshare Coding Plan",
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
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          context_length: 400000,
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ccsub",
    icon: "ccsub",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "ccsub" },
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
    name: "ETok.ai",
    websiteUrl: "https://etok.ai",
    apiKeyUrl: "https://etok.ai",
    settingsConfig: {
      name: "etok",
      base_url: "https://api.etok.ai",
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
    partnerPromotionKey: "etok",
    icon: "etok",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-5", provider: "etok" },
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
  },
  {
    name: "SudoCode.chat",
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
      models: [{ id: "gpt-5.6-sol", name: "GPT-5.6 Sol" }],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "xycai",
    icon: "xycai",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "xycai" },
    },
  },
  // ===== 非赞助商预设：应用内展示按显示名排序，此处文件顺序不影响展示 =====
  {
    name: "Amux",
    websiteUrl: "https://amux.ai",
    apiKeyUrl: "https://amux.ai",
    settingsConfig: {
      name: "amux",
      base_url: "https://api.amux.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "gpt-5.6-sol", name: "GPT-5.6 Sol" }],
    },
    category: "aggregator",
    icon: "amux",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "amux" },
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
          id: "anthropic/claude-haiku-4-5",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
        {
          id: "openai/gpt-5.6-sol",
          name: "GPT-5.6 Sol",
          context_length: 400000,
        },
        {
          id: "google/gemini-3.6-flash",
          name: "Gemini 3.6 Flash",
          context_length: 1000000,
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
          id: "deepseek-v4-flash",
          name: "DeepSeek V4 Flash",
          context_length: 1000000,
        },
      ],
    },
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#4D6BFE",
    suggestedDefaults: {
      model: { default: "deepseek-v4-flash", provider: "deepseek" },
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
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    settingsConfig: {
      name: "zhipu_glm",
      base_url: "https://open.bigmodel.cn/api/coding/paas/v4",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "glm-5.1", name: "GLM-5.1" }],
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
    suggestedDefaults: {
      model: { default: "glm-5.1", provider: "zhipu_glm" },
    },
  },
  {
    name: "Zhipu GLM en",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe?ic=8JVLJQFSKB",
    settingsConfig: {
      name: "zhipu_glm_en",
      base_url: "https://api.z.ai/api/coding/paas/v4",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "glm-5.1", name: "GLM-5.1" }],
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
    suggestedDefaults: {
      model: { default: "glm-5.1", provider: "zhipu_glm_en" },
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
        { id: "minimax-m2.5", name: "MiniMax M2.5" },
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
        { id: "minimax-m2.5", name: "MiniMax M2.5" },
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
    name: "Bailian",
    websiteUrl: "https://bailian.console.aliyun.com",
    settingsConfig: {
      name: "bailian",
      base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "qwen3-coder-plus", name: "Qwen3 Coder Plus" },
        { id: "qwen3-max", name: "Qwen3 Max" },
      ],
    },
    category: "cn_official",
    icon: "bailian",
    iconColor: "#624AFF",
    suggestedDefaults: {
      model: { default: "qwen3-coder-plus", provider: "bailian" },
    },
  },
  {
    name: "Bailian For Coding",
    websiteUrl: "https://bailian.console.aliyun.com",
    settingsConfig: {
      name: "bailian_coding",
      base_url: "https://coding.dashscope.aliyuncs.com/apps/anthropic",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "qwen3-coder-plus", name: "Qwen3 Coder Plus" },
        { id: "qwen3-max", name: "Qwen3 Max" },
      ],
    },
    category: "cn_official",
    icon: "bailian",
    iconColor: "#624AFF",
    suggestedDefaults: {
      model: { default: "qwen3-coder-plus", provider: "bailian_coding" },
    },
  },
  // ===== QwenCloud（DashScope 国际站）=====
  // 三条线都用 anthropic_messages；地址不带 /v1（与官方 hermes 文档一致，
  // 这点和 OpenCode / OpenClaw 的 /apps/anthropic/v1 不同，勿互相照搬）。
  {
    name: "QwenCloud",
    websiteUrl: "https://www.qwencloud.com",
    apiKeyUrl: "https://home.qwencloud.com/api-keys",
    settingsConfig: {
      name: "qwencloud",
      base_url: "https://dashscope-intl.aliyuncs.com/apps/anthropic",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "qwen3.7-max", name: "Qwen3.7 Max", context_length: 1000000 },
        { id: "qwen3.7-plus", name: "Qwen3.7 Plus", context_length: 1000000 },
        { id: "qwen3.6-plus", name: "Qwen3.6 Plus", context_length: 1000000 },
      ],
    },
    category: "cn_official",
    icon: "qwen",
    iconColor: "#6336E7",
    suggestedDefaults: {
      model: { default: "qwen3.7-max", provider: "qwencloud" },
    },
  },
  {
    name: "QwenCloud For Coding",
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
    icon: "qwen",
    iconColor: "#6336E7",
    suggestedDefaults: {
      model: { default: "qwen3.7-plus", provider: "qwencloud_coding" },
    },
  },
  {
    name: "QwenCloud Token Plan",
    websiteUrl: "https://www.qwencloud.com",
    apiKeyUrl: "https://home.qwencloud.com/api-keys",
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
    icon: "qwen",
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
      models: [{ id: "step-3.5-flash", name: "Step 3.5 Flash" }],
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
    name: "KAT-Coder",
    websiteUrl: "https://console.streamlake.ai",
    apiKeyUrl: "https://console.streamlake.ai/console/api-key",
    settingsConfig: {
      name: "kat_coder",
      base_url:
        "https://vanchin.streamlake.ai/api/gateway/v1/endpoints/${ENDPOINT_ID}/claude-code-proxy",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "KAT-Coder-Pro V1", name: "KAT-Coder Pro V1" },
        { id: "KAT-Coder-Air V1", name: "KAT-Coder Air V1" },
      ],
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
    suggestedDefaults: {
      model: { default: "KAT-Coder-Pro V1", provider: "kat_coder" },
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
    websiteUrl: "https://platform.minimaxi.com",
    apiKeyUrl: "https://platform.minimaxi.com/subscribe/coding-plan",
    settingsConfig: {
      name: "minimax",
      base_url: "https://api.minimaxi.com/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "MiniMax-M2.7", name: "MiniMax M2.7" }],
    },
    category: "cn_official",
    partnerPromotionKey: "minimax_cn",
    theme: { backgroundColor: "#f64551", textColor: "#FFFFFF" },
    icon: "minimax",
    iconColor: "#FF6B6B",
    suggestedDefaults: {
      model: { default: "MiniMax-M2.7", provider: "minimax" },
    },
  },
  {
    name: "MiniMax en",
    websiteUrl: "https://platform.minimax.io",
    apiKeyUrl: "https://platform.minimax.io/subscribe/coding-plan",
    settingsConfig: {
      name: "minimax_en",
      base_url: "https://api.minimax.io/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "MiniMax-M2.7", name: "MiniMax M2.7" }],
    },
    category: "cn_official",
    partnerPromotionKey: "minimax_en",
    theme: { backgroundColor: "#f64551", textColor: "#FFFFFF" },
    icon: "minimax",
    iconColor: "#FF6B6B",
    suggestedDefaults: {
      model: { default: "MiniMax-M2.7", provider: "minimax_en" },
    },
  },
  {
    name: "BaiLing",
    websiteUrl: "https://alipaytbox.yuque.com/sxs0ba/ling/get_started",
    settingsConfig: {
      name: "bailing",
      base_url: "https://api.tbox.cn/api/anthropic",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [{ id: "Ling-2.5-1T", name: "Ling 2.5 1T" }],
    },
    category: "cn_official",
    suggestedDefaults: {
      model: { default: "Ling-2.5-1T", provider: "bailing" },
    },
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
      models: [{ id: "gpt-5.6-sol", name: "GPT-5.6 Sol" }],
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
      models: [{ id: "zai-org/glm-5.1", name: "Zai-Org / GLM-5.1" }],
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
      models: [{ id: "moonshotai/kimi-k2.5", name: "Moonshot Kimi K2.5" }],
    },
    category: "aggregator",
    icon: "nvidia",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "moonshotai/kimi-k2.5", provider: "nvidia" },
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
    websiteUrl: "https://platform.xiaomimimo.com",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/api-keys",
    settingsConfig: {
      name: "xiaomi_mimo",
      base_url: "https://api.xiaomimimo.com/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "mimo-v2.5-pro", name: "MiMo v2.5 Pro" }],
    },
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "mimo-v2.5-pro", provider: "xiaomi_mimo" },
    },
  },
  {
    name: "Xiaomi MiMo Token Plan (China)",
    websiteUrl: "https://platform.xiaomimimo.com/#/token-plan",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/plan-manage",
    settingsConfig: {
      name: "xiaomi_mimo_token_plan",
      base_url: "https://token-plan-cn.xiaomimimo.com/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        { id: "mimo-v2.5-pro", name: "MiMo v2.5 Pro" },
        { id: "mimo-v2.5", name: "MiMo v2.5" },
      ],
    },
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "mimo-v2.5-pro", provider: "xiaomi_mimo_token_plan" },
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
      base_url: "https://api.aicodewith.ai/chatgpt/v1",
      api_key: "",
      api_mode: "codex_responses",
      models: [
        {
          id: "gpt-5.6-sol",
          name: "GPT-5.6 Sol",
        },
      ],
    },
    category: "aggregator",
    icon: "aicodewith",
    iconColor: "#3A3B40",
    suggestedDefaults: {
      model: { default: "gpt-5.6-sol", provider: "aicodewith" },
    },
  },
];
