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
 *   anthropic/claude-opus-4-7:
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
  {
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    settingsConfig: {
      name: "shengsuanyun",
      base_url: "https://router.shengsuanyun.com/api/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "openai/gpt-5.4", name: "GPT-5.4" }],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "shengsuanyun",
    icon: "shengsuanyun",
    suggestedDefaults: {
      model: { default: "openai/gpt-5.4", provider: "shengsuanyun" },
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
          id: "anthropic/claude-opus-4-7",
          name: "Claude Opus 4.7",
          context_length: 1000000,
        },
        {
          id: "anthropic/claude-sonnet-4-6",
          name: "Claude Sonnet 4.6",
          context_length: 1000000,
        },
        {
          id: "anthropic/claude-haiku-4-5",
          name: "Claude Haiku 4.5",
          context_length: 200000,
        },
        {
          id: "openai/gpt-5.4",
          name: "GPT-5.4",
          context_length: 400000,
        },
        {
          id: "google/gemini-3-pro",
          name: "Gemini 3 Pro",
          context_length: 1000000,
        },
      ],
    },
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6366F1",
    suggestedDefaults: {
      model: { default: "anthropic/claude-opus-4-7", provider: "openrouter" },
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
          id: "deepseek-chat",
          name: "DeepSeek V3.2",
          context_length: 128000,
        },
        {
          id: "deepseek-reasoner",
          name: "DeepSeek R1",
          context_length: 128000,
        },
      ],
    },
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#4D6BFE",
    suggestedDefaults: {
      model: { default: "deepseek-chat", provider: "deepseek" },
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

  // ===== 以下为从 Claude 应用预设同步而来的供应商 =====
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
      base_url: "https://open.bigmodel.cn/api/paas/v4",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "glm-5", name: "GLM-5" }],
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
    suggestedDefaults: {
      model: { default: "glm-5", provider: "zhipu_glm" },
    },
  },
  {
    name: "Zhipu GLM en",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe?ic=8JVLJQFSKB",
    settingsConfig: {
      name: "zhipu_glm_en",
      base_url: "https://api.z.ai/api/paas/v4",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "glm-5", name: "GLM-5" }],
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
    suggestedDefaults: {
      model: { default: "glm-5", provider: "zhipu_glm_en" },
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
  {
    name: "Kimi",
    websiteUrl: "https://platform.moonshot.cn/console",
    settingsConfig: {
      name: "kimi",
      base_url: "https://api.moonshot.cn/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "kimi-k2.6", name: "Kimi K2.6" }],
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
    suggestedDefaults: {
      model: { default: "kimi-k2.6", provider: "kimi" },
    },
  },
  {
    name: "Kimi For Coding",
    websiteUrl: "https://www.kimi.com/coding/docs/",
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
      models: [{ id: "ZhipuAI/GLM-5", name: "ZhipuAI / GLM-5" }],
    },
    category: "aggregator",
    icon: "modelscope",
    iconColor: "#624AFF",
    suggestedDefaults: {
      model: { default: "ZhipuAI/GLM-5", provider: "modelscope" },
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
      models: [{ id: "LongCat-Flash-Chat", name: "LongCat Flash Chat" }],
    },
    category: "cn_official",
    icon: "longcat",
    iconColor: "#29E154",
    suggestedDefaults: {
      model: { default: "LongCat-Flash-Chat", provider: "longcat" },
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
    isPartner: true,
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
    isPartner: true,
    partnerPromotionKey: "minimax_en",
    theme: { backgroundColor: "#f64551", textColor: "#FFFFFF" },
    icon: "minimax",
    iconColor: "#FF6B6B",
    suggestedDefaults: {
      model: { default: "MiniMax-M2.7", provider: "minimax_en" },
    },
  },
  {
    name: "DouBaoSeed",
    websiteUrl: "https://www.volcengine.com/product/doubao",
    apiKeyUrl: "https://www.volcengine.com/product/doubao",
    settingsConfig: {
      name: "doubao_seed",
      base_url: "https://ark.cn-beijing.volces.com/api/coding",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        {
          id: "doubao-seed-2-0-code-preview-latest",
          name: "Doubao Seed 2.0 Code Preview",
        },
      ],
    },
    category: "cn_official",
    icon: "doubao",
    iconColor: "#3370FF",
    suggestedDefaults: {
      model: {
        default: "doubao-seed-2-0-code-preview-latest",
        provider: "doubao_seed",
      },
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
      models: [{ id: "gpt-5.4", name: "GPT-5.4" }],
    },
    category: "aggregator",
    icon: "aihubmix",
    iconColor: "#006FFB",
    suggestedDefaults: {
      model: { default: "gpt-5.4", provider: "aihubmix" },
    },
  },
  {
    name: "SiliconFlow",
    websiteUrl: "https://siliconflow.cn",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/drGuwc9k",
    settingsConfig: {
      name: "siliconflow",
      base_url: "https://api.siliconflow.cn/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [
        {
          id: "Pro/MiniMaxAI/MiniMax-M2.7",
          name: "Pro / MiniMax M2.7",
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
        default: "Pro/MiniMaxAI/MiniMax-M2.7",
        provider: "siliconflow",
      },
    },
  },
  {
    name: "SiliconFlow en",
    websiteUrl: "https://siliconflow.com",
    apiKeyUrl: "https://cloud.siliconflow.cn/i/drGuwc9k",
    settingsConfig: {
      name: "siliconflow_en",
      base_url: "https://api.siliconflow.com/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "MiniMaxAI/MiniMax-M2.7", name: "MiniMax M2.7" }],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "siliconflow",
    icon: "siliconflow",
    iconColor: "#000000",
    suggestedDefaults: {
      model: {
        default: "MiniMaxAI/MiniMax-M2.7",
        provider: "siliconflow_en",
      },
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
      models: [{ id: "gpt-5.4", name: "GPT-5.4" }],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "dmxapi",
    suggestedDefaults: {
      model: { default: "gpt-5.4", provider: "dmxapi" },
    },
  },
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.com",
    apiKeyUrl: "https://www.packyapi.com/register?aff=cc-switch",
    settingsConfig: {
      name: "packycode",
      base_url: "https://www.packyapi.com",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
        { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "packycode",
    icon: "packycode",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "packycode" },
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
        { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
        { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "cubence",
    icon: "cubence",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "cubence" },
    },
  },
  {
    name: "AIGoCode",
    websiteUrl: "https://aigocode.com",
    apiKeyUrl: "https://aigocode.com/invite/CC-SWITCH",
    settingsConfig: {
      name: "aigocode",
      base_url: "https://api.aigocode.com",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
        { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aigocode",
    icon: "aigocode",
    iconColor: "#5B7FFF",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "aigocode" },
    },
  },
  {
    name: "RightCode",
    websiteUrl: "https://www.right.codes",
    apiKeyUrl: "https://www.right.codes/register?aff=CCSWITCH",
    settingsConfig: {
      name: "rightcode",
      base_url: "https://www.right.codes/claude",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
        { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "rightcode",
    icon: "rc",
    iconColor: "#E96B2C",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "rightcode" },
    },
  },
  {
    name: "AICodeMirror",
    websiteUrl: "https://www.aicodemirror.com",
    apiKeyUrl: "https://www.aicodemirror.com/register?invitecode=9915W3",
    settingsConfig: {
      name: "aicodemirror",
      base_url: "https://api.aicodemirror.com/api/claudecode",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
        { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicodemirror",
    icon: "aicodemirror",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "aicodemirror" },
    },
  },
  {
    name: "AICoding",
    websiteUrl: "https://aicoding.sh",
    apiKeyUrl: "https://aicoding.sh/i/CCSWITCH",
    settingsConfig: {
      name: "aicoding",
      base_url: "https://api.aicoding.sh",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
        { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicoding",
    icon: "aicoding",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "aicoding" },
    },
  },
  {
    name: "CrazyRouter",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    settingsConfig: {
      name: "crazyrouter",
      base_url: "https://crazyrouter.com",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
        { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "crazyrouter",
    icon: "crazyrouter",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "crazyrouter" },
    },
  },
  {
    name: "SSSAiCode",
    websiteUrl: "https://www.sssaicode.com",
    apiKeyUrl: "https://www.sssaicode.com/register?ref=DCP0SM",
    settingsConfig: {
      name: "sssaicode",
      base_url: "https://node-hk.sssaicode.com/api",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
        { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "sssaicode",
    icon: "sssaicode",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "sssaicode" },
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
      models: [{ id: "gpt-5.4", name: "GPT-5.4" }],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ucloud",
    icon: "ucloud",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "gpt-5.4", provider: "compshare" },
    },
  },
  {
    name: "Micu",
    websiteUrl: "https://www.openclaudecode.cn",
    apiKeyUrl: "https://www.openclaudecode.cn/register?aff=aOYQ",
    settingsConfig: {
      name: "micu",
      base_url: "https://www.openclaudecode.cn",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
        { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "micu",
    icon: "micu",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "micu" },
    },
  },
  {
    name: "CTok.ai",
    websiteUrl: "https://ctok.ai",
    apiKeyUrl: "https://ctok.ai",
    settingsConfig: {
      name: "ctok",
      base_url: "https://api.ctok.ai",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
        { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "ctok",
    icon: "ctok",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "ctok" },
    },
  },
  {
    name: "DDSHub",
    websiteUrl: "https://www.ddshub.cc",
    apiKeyUrl: "https://ddshub.short.gy/ccswitch",
    settingsConfig: {
      name: "ddshub",
      base_url: "https://www.ddshub.cc",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
        { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "ddshub",
    icon: "dds",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "ddshub" },
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
        { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
        { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    icon: "eflowcode",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "eflowcode" },
    },
  },
  {
    name: "LionCCAPI",
    websiteUrl: "https://vibecodingapi.ai",
    settingsConfig: {
      name: "lionccapi",
      base_url: "https://vibecodingapi.ai",
      api_key: "",
      api_mode: "anthropic_messages",
      models: [
        { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
        { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
        { id: "claude-haiku-4-5-20251001", name: "Claude Haiku 4.5" },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "lionccapi",
    icon: "lioncc",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "lionccapi" },
    },
  },
  {
    name: "LemonData",
    websiteUrl: "https://lemondata.cc",
    apiKeyUrl: "https://lemondata.cc/r/FFX1ZDUP",
    settingsConfig: {
      name: "lemondata",
      base_url: "https://api.lemondata.cc/v1",
      api_key: "",
      api_mode: "chat_completions",
      models: [{ id: "gpt-5.4", name: "GPT-5.4" }],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "lemondata",
    icon: "lemondata",
    suggestedDefaults: {
      model: { default: "gpt-5.4", provider: "lemondata" },
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
        { id: "openai/gpt-5.4", name: "GPT-5.4" },
        { id: "openai/gpt-5.4-mini", name: "GPT-5.4 mini" },
        { id: "openai/gpt-5.4-nano", name: "GPT-5.4 nano" },
      ],
    },
    category: "aggregator",
    suggestedDefaults: {
      model: {
        default: "openai/gpt-5.4",
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
      models: [{ id: "zai-org/glm-5", name: "Zai-Org / GLM-5" }],
    },
    category: "aggregator",
    icon: "novita",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "zai-org/glm-5", provider: "novita" },
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
        { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
        { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
        {
          id: "claude-haiku-4-5-20251001",
          name: "Claude Haiku 4.5",
        },
      ],
    },
    category: "aggregator",
    icon: "pipellm",
    suggestedDefaults: {
      model: { default: "claude-opus-4-7", provider: "pipellm" },
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
      models: [{ id: "mimo-v2-pro", name: "MiMo v2 Pro" }],
    },
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
    suggestedDefaults: {
      model: { default: "mimo-v2-pro", provider: "xiaomi_mimo" },
    },
  },
];
