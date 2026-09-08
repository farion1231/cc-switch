import type { ProviderCategory } from "@/types";
import type { PresetTheme } from "./claudeProviderPresets";
import {
  getPiModelCatalogReference,
  piModel,
  type PiCatalogModel,
} from "./piModelCatalog";
import {
  getPiThinkingProfile,
  resolvePiThinkingProfile,
  type PiThinkingLevelMap,
} from "./piThinkingProfiles";

export type PiApiFormat =
  | "openai-completions"
  | "openai-responses"
  | "anthropic-messages"
  | "google-generative-ai"
  | "bedrock-converse-stream";

export type PiPresetModel = PiCatalogModel & {
  thinkingLevelMap?: PiThinkingLevelMap;
  compat?: Record<string, unknown>;
};

export interface PiProviderPreset {
  name: string;
  nameKey?: string;
  providerKey: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: {
    name: string;
    baseUrl: string;
    api: PiApiFormat;
    apiKey: string;
    headers?: Record<string, string>;
    compat?: Record<string, unknown>;
    models: PiPresetModel[];
  };
  category?: ProviderCategory;
  isPartner?: boolean;
  primePartner?: boolean;
  partnerPromotionKey?: string;
  theme?: PresetTheme;
  icon?: string;
  iconColor?: string;
}

const OPENAI_COMPLETIONS_COMPAT = {
  supportsStore: false,
  supportsDeveloperRole: false,
  maxTokensField: "max_tokens",
} as const;

const DEEPSEEK_THINKING_COMPAT = {
  ...OPENAI_COMPLETIONS_COMPAT,
  requiresReasoningContentOnAssistantMessages: true,
  thinkingFormat: "deepseek",
} as const;

// 1823/132248: thinking.type defaults to "enabled"; disabling requires an explicit
// {"type":"disabled"}. The same doc says reasoning_content need not be echoed back
// on multi-turn, so we don't reuse DEEPSEEK_THINKING_COMPAT wholesale.
const TENCENT_DEEPSEEK_THINKING_COMPAT = {
  ...OPENAI_COMPLETIONS_COMPAT,
  thinkingFormat: "deepseek",
} as const;

const XIAOMI_THINKING_COMPAT = {
  requiresReasoningContentOnAssistantMessages: true,
  thinkingFormat: "deepseek",
} as const;

const KIMI_K3_COMPAT = {
  supportsStore: false,
  supportsDeveloperRole: false,
  supportsReasoningEffort: true,
  maxTokensField: "max_tokens",
  supportsStrictMode: false,
  thinkingFormat: "openai",
  requiresReasoningContentOnAssistantMessages: true,
  deferredToolsMode: "kimi",
} as const;

/**
 * Pi-native provider catalog.
 *
 * This list is independently maintained because provider protocol, endpoint
 * roots and model capabilities are application-specific. It was initially
 * aligned with the OpenCode catalog, but Pi does not import or derive from
 * another application's presets at runtime.
 */
const piProviderPresetDefinitions: PiProviderPreset[] = [
  {
    name: "Kimi",
    providerKey: "cc-switch-kimi",
    websiteUrl: "https://platform.kimi.com?aff=cc-switch",
    apiKeyUrl: "https://platform.kimi.com/console/api-keys?aff=cc-switch",
    settingsConfig: {
      name: "Kimi",
      baseUrl: "https://api.moonshot.cn/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("moonshotai/kimi-k2.7-code", {
          id: "kimi-k2.7-code",
          thinkingProfile: "offUnsupported",
        }),
        {
          ...piModel("moonshotai/kimi-k3", {
            id: "kimi-k3",
            thinkingProfile: "kimi3",
          }),
          compat: { ...KIMI_K3_COMPAT },
        },
      ],
    },
    category: "cn_official",
    primePartner: true,
    partnerPromotionKey: "kimi",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "Kimi For Coding",
    providerKey: "cc-switch-kimi-for-coding",
    websiteUrl: "https://www.kimi.com/code/?aff=cc-switch",
    apiKeyUrl: "https://platform.kimi.com/console/api-keys?aff=cc-switch",
    settingsConfig: {
      name: "Kimi For Coding",
      baseUrl: "https://api.kimi.com/coding",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("moonshotai/kimi-k2.7-code", {
          id: "kimi-for-coding",
          name: "Kimi For Coding",
          maxTokens: 32768,
        }),
      ],
    },
    category: "cn_official",
    primePartner: true,
    icon: "kimi",
    iconColor: "#6366F1",
  },
  {
    name: "PackyCode",
    providerKey: "cc-switch-packy-code",
    websiteUrl: "https://www.packyapi.ai",
    apiKeyUrl: "https://www.packyapi.ai/register?aff=cc-switch",
    settingsConfig: {
      name: "PackyCode",
      baseUrl: "https://www.packyapi.ai",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "packycode",
    icon: "packycode",
  },
  {
    name: "ZetaAPI",
    providerKey: "cc-switch-zeta-api",
    websiteUrl: "https://zetaapi.ai",
    apiKeyUrl: "https://zetaapi.ai/go/u117",
    settingsConfig: {
      name: "ZetaAPI",
      baseUrl: "https://api.zetaapi.ai/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.6-sol", {
          id: "gpt-5.6-sol",
        }),
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "zetaapi",
    icon: "zetaapi",
  },
  {
    name: "APINebula",
    providerKey: "cc-switch-apinebula",
    websiteUrl: "https://apinebula.ai",
    apiKeyUrl: "https://apinebula.ai/VjM74M",
    settingsConfig: {
      name: "APINebula",
      baseUrl: "https://apinebula.ai/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.6-sol", {
          id: "gpt-5.6-sol",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apinebula",
    icon: "apinebula",
  },
  {
    name: "AICodeMirror",
    providerKey: "cc-switch-aicode-mirror",
    websiteUrl: "https://www.aicodemirror.ai",
    apiKeyUrl: "https://www.aicodemirror.ai/register?invitecode=9915W3",
    settingsConfig: {
      name: "AICodeMirror",
      baseUrl: "https://api.aicodemirror.ai/api/claudecode",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicodemirror",
    icon: "aicodemirror",
    iconColor: "#000000",
  },
  {
    name: "FennoAI",
    providerKey: "cc-switch-fenno-ai",
    websiteUrl: "https://api.fenno.ai",
    apiKeyUrl:
      "https://api.fenno.ai/register?redirect=/purchase?tab=subscription%26group=16&aff=P9MR3D3PLCNL",
    settingsConfig: {
      name: "FennoAI",
      baseUrl: "https://api.fenno.ai/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.6-sol", {
          id: "gpt-5.6-sol",
        }),
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "fenno",
    icon: "fenno",
  },
  {
    name: "RunAPI",
    providerKey: "cc-switch-run-api",
    websiteUrl: "https://runapi.co",
    apiKeyUrl: "https://runapi.co/register?aff=iOKB",
    settingsConfig: {
      name: "RunAPI",
      baseUrl: "https://runapi.co",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
        piModel("anthropic/claude-haiku-4.5", {
          id: "claude-haiku-4-5",
        }),
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "runapi",
    icon: "runapi",
  },
  {
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    providerKey: "cc-switch-shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    settingsConfig: {
      name: "Shengsuanyun",
      baseUrl: "https://router.shengsuanyun.com/api",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-opus-5", {
          id: "anthropic/claude-opus-5",
        }),
        piModel("anthropic/claude-sonnet-5", {
          id: "anthropic/claude-sonnet-5",
        }),
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "shengsuanyun",
    icon: "shengsuanyun",
  },
  {
    name: "AIGoCode",
    providerKey: "cc-switch-aigo-code",
    websiteUrl: "https://aigocode.app",
    apiKeyUrl: "https://aigocode.app/invite/CC-SWITCH",
    settingsConfig: {
      name: "AIGoCode",
      baseUrl: "https://api.aigocode.app",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aigocode",
    icon: "aigocode",
    iconColor: "#5B7FFF",
  },
  {
    name: "Qiniu",
    nameKey: "providerForm.presets.qiniu",
    providerKey: "cc-switch-qiniu",
    websiteUrl: "https://s.qiniu.com/nMvAvy",
    apiKeyUrl: "https://s.qiniu.com/nMvAvy",
    settingsConfig: {
      name: "Qiniu",
      baseUrl: "https://api.qnaigc.com/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.6-sol", {
          id: "gpt-5.6-sol",
        }),
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "qiniu",
    icon: "qiniu",
  },
  {
    name: "AICoding",
    providerKey: "cc-switch-aicoding",
    websiteUrl: "https://aicoding.inc",
    apiKeyUrl: "https://aicoding.inc/i/CCSWITCH",
    settingsConfig: {
      name: "AICoding",
      baseUrl: "https://api.aicoding.inc",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aicoding",
    icon: "aicoding",
    iconColor: "#000000",
  },
  {
    name: "SubRouter",
    providerKey: "cc-switch-sub-router",
    websiteUrl: "https://subrouter.ai",
    apiKeyUrl: "https://subrouter.ai/register?aff=l3ri",
    settingsConfig: {
      name: "SubRouter",
      baseUrl: "https://subrouter.ai/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.6-sol", {
          id: "gpt-5.6-sol",
        }),
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "subrouter",
    icon: "subrouter",
  },
  {
    name: "APIKEY.FUN",
    providerKey: "cc-switch-apikey-fun",
    websiteUrl: "https://apikey.fun",
    apiKeyUrl: "https://apikey.fun/register?aff=CCSwitch",
    settingsConfig: {
      name: "APIKEY.FUN",
      baseUrl: "https://api.apikey.fun",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-haiku-4.5", {
          id: "claude-haiku-4-5",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "apikeyfun",
    icon: "apikeyfun",
  },
  {
    name: "9527CODE",
    providerKey: "cc-switch-9527-code",
    websiteUrl: "https://9527.codes",
    apiKeyUrl: "https://9527.codes/register?aff=e5zI",
    settingsConfig: {
      name: "9527CODE",
      baseUrl: "https://9527.codes",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-haiku-4.5", {
          id: "claude-haiku-4-5",
        }),
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "9527code",
    icon: "9527code",
  },
  {
    name: "Code0",
    providerKey: "cc-switch-code0",
    websiteUrl: "https://code0.ai",
    apiKeyUrl: "https://code0.ai/agent/register/B2XHxGjGmRvqgznY",
    settingsConfig: {
      name: "Code0",
      baseUrl: "https://code0.ai/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.6-sol", {
          id: "gpt-5.6-sol",
        }),
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "code0",
    icon: "code0",
  },
  {
    name: "TeamoRouter",
    providerKey: "cc-switch-teamo-router",
    websiteUrl: "https://teamorouter.cn",
    apiKeyUrl:
      "https://teamorouter.cn/?utm_source=cc_switch&utm_medium=referral&utm_campaign=ai_directory",
    settingsConfig: {
      name: "TeamoRouter",
      baseUrl: "https://api.teamorouter.cn/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.6-sol", {
          id: "gpt-5.6-sol",
        }),
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "teamorouter",
    icon: "teamorouter",
  },
  {
    name: "PPIO",
    providerKey: "cc-switch-ppio",
    websiteUrl: "https://ppio.com",
    apiKeyUrl: "https://ppio.com/activity/ccswitch",
    settingsConfig: {
      name: "PPIO",
      baseUrl: "https://api.ppio.com/openai/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek/deepseek-v4-flash-0731",
            name: "Deepseek V4 Flash 0731",
            contextWindow: 1_048_576,
            maxTokens: 393_216,
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...DEEPSEEK_THINKING_COMPAT },
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ppio",
    icon: "ppio",
    iconColor: "#2874FF",
  },
  {
    name: "ClaudeCN",
    providerKey: "cc-switch-claude-cn",
    websiteUrl: "https://claudecn.top",
    apiKeyUrl: "https://claudecn.ai/register?aff=HEL9",
    settingsConfig: {
      name: "ClaudeCN",
      baseUrl: "https://claudecn.top",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
        piModel("anthropic/claude-haiku-4.5", {
          id: "claude-haiku-4-5",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "claudecn",
    icon: "claudecn",
  },
  {
    name: "火山Agentplan",
    providerKey: "cc-switch-agentplan",
    websiteUrl:
      "https://www.volcengine.com/activity/codingplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://www.volcengine.com/activity/codingplan?ac=MMAP8JTTCAQ2&rc=6J6FV5N2&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    settingsConfig: {
      name: "火山Agentplan",
      baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("volcengine/ark-code-latest", {
          id: "ark-code-latest",
        }),
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "volcengine_agentplan",
    icon: "huoshan",
    iconColor: "#3370FF",
  },
  {
    name: "BytePlus",
    providerKey: "cc-switch-byte-plus",
    websiteUrl:
      "https://www.byteplus.com/en/product/modelark?utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://www.byteplus.com/en/product/modelark?utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    settingsConfig: {
      name: "BytePlus",
      baseUrl: "https://ark.ap-southeast.bytepluses.com/api/coding/v3",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("volcengine/ark-code-latest", {
          id: "ark-code-latest",
        }),
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "byteplus",
    icon: "byteplus",
    iconColor: "#3370FF",
  },
  {
    name: "DouBaoSeed",
    providerKey: "cc-switch-dou-bao-seed",
    websiteUrl:
      "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey?apikey=%7B%7D&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    apiKeyUrl:
      "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey?apikey=%7B%7D&utm_campaign=hw&utm_content=ccswitch&utm_medium=devrel_tool_web&utm_source=OWO&utm_term=ccswitch",
    settingsConfig: {
      name: "DouBaoSeed",
      baseUrl: "https://ark.cn-beijing.volces.com/api/v3",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("volcengine/doubao-seed-2.1-pro", {
          id: "doubao-seed-2-1-pro-260628",
        }),
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "doubaoseed",
    icon: "doubao",
    iconColor: "#3370FF",
  },
  {
    name: "A6API",
    providerKey: "cc-switch-a6-api",
    websiteUrl: "https://www.a6api.com",
    apiKeyUrl: "https://a6api.com/register?aff=AqNr",
    settingsConfig: {
      name: "A6API",
      baseUrl: "https://api.a6api.com/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.6-sol", {
          id: "gpt-5.6-sol",
        }),
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "a6api",
    icon: "a6api",
  },
  {
    name: "AtlasCloud",
    providerKey: "cc-switch-atlas-cloud",
    websiteUrl: "https://www.atlascloud.ai/console/coding-plan",
    apiKeyUrl: "https://www.atlascloud.ai/console/coding-plan",
    settingsConfig: {
      name: "AtlasCloud",
      baseUrl: "https://api.atlascloud.ai/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("zai/glm-5.1", {
          id: "zai-org/glm-5.1",
          name: "GLM 5.1",
        }),
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "atlascloud",
    icon: "atlascloud",
  },
  {
    name: "CCSub",
    providerKey: "cc-switch-ccsub",
    websiteUrl: "https://www.ccsub.net",
    apiKeyUrl: "https://www.ccsub.net/register?ref=Y6Z8DXEA",
    settingsConfig: {
      name: "CCSub",
      baseUrl: "https://www.ccsub.net/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.6-sol", {
          id: "gpt-5.6-sol",
        }),
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "ccsub",
    icon: "ccsub",
  },
  {
    name: "SSSAiCode",
    providerKey: "cc-switch-sssai-code",
    websiteUrl: "https://sssaicodeapi.com",
    apiKeyUrl: "https://sssaicodeapi.com/register?ref=DCP0SM",
    settingsConfig: {
      name: "SSSAiCode",
      baseUrl: "https://node-hk.sssaicodeapi.com/api",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "sssaicode",
    icon: "sssaicode",
    iconColor: "#000000",
  },
  {
    name: "SoleAPI",
    providerKey: "cc-switch-sole-api",
    websiteUrl: "https://soleapi.com",
    apiKeyUrl: "https://soleapi.com/r/ccswitch",
    settingsConfig: {
      name: "SoleAPI",
      baseUrl: "https://soleapi.com",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-haiku-4.5-20251001", {
          id: "claude-haiku-4-5-20251001",
        }),
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "soleapi",
    icon: "soleapi",
  },
  {
    name: "Micu",
    providerKey: "cc-switch-micu",
    websiteUrl: "https://www.micuapi.ai",
    apiKeyUrl: "https://www.micuapi.ai/register?aff=aOYQ",
    settingsConfig: {
      name: "Micu",
      baseUrl: "https://www.micuapi.ai",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "micu",
    icon: "micu",
    iconColor: "#000000",
  },
  {
    name: "RightCode",
    providerKey: "cc-switch-right-code",
    websiteUrl: "https://www.rightapi.ai",
    apiKeyUrl: "https://www.rightapi.ai/register?aff=CCSWITCH",
    settingsConfig: {
      name: "RightCode",
      baseUrl: "https://www.rightapi.ai/codex/v1",
      api: "openai-responses",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.6-sol", {
          id: "gpt-5.6-sol",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "rightcode",
    icon: "rc",
    iconColor: "#E96B2C",
  },
  {
    name: "ETok.ai",
    providerKey: "cc-switch-etok-ai",
    websiteUrl: "https://etok.ai",
    apiKeyUrl: "https://etok.ai",
    settingsConfig: {
      name: "ETok",
      baseUrl: "https://api.etok.ai",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "etok",
    icon: "etok",
    iconColor: "#000000",
  },
  {
    name: "Cubence",
    providerKey: "cc-switch-cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    settingsConfig: {
      name: "Cubence",
      baseUrl: "https://api.cubence.com",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "cubence",
    icon: "cubence",
    iconColor: "#000000",
  },
  {
    name: "CrazyRouter",
    providerKey: "cc-switch-crazy-router",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    settingsConfig: {
      name: "CrazyRouter",
      baseUrl: "https://cn.crazyrouter.com",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "crazyrouter",
    icon: "crazyrouter",
    iconColor: "#000000",
  },
  {
    name: "DMXAPI",
    providerKey: "cc-switch-dmxapi",
    websiteUrl: "https://www.dmxapi.cn",
    apiKeyUrl: "https://www.dmxapi.cn",
    settingsConfig: {
      name: "DMXAPI",
      baseUrl: "https://www.dmxapi.cn",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "dmxapi",
  },
  {
    name: "SudoCode.chat",
    providerKey: "cc-switch-sudo-code-chat",
    websiteUrl: "https://sudocode.chat",
    apiKeyUrl:
      "https://sudocode.chat/sign-up?aff=CC-SWITCH&utm_source=cc-switch&utm_medium=sponsor&utm_campaign=ccswitch",
    settingsConfig: {
      name: "SudoCode.chat",
      baseUrl: "https://api.sudocode.chat/v1",
      api: "openai-responses",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.6-sol", {
          id: "gpt-5.6-sol",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "sudocode",
    icon: "sudocode",
  },
  {
    name: "SudoCode.us",
    providerKey: "cc-switch-sudo-code-us",
    websiteUrl: "https://sudocode.us",
    apiKeyUrl: "https://sudocode.us",
    settingsConfig: {
      name: "SudoCode.us",
      baseUrl: "https://sudocode.us/v1",
      api: "openai-responses",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.6-sol", {
          id: "gpt-5.6-sol",
        }),
      ],
    },
    category: "third_party",
    isPartner: true,
    icon: "sudocode-us",
  },
  {
    name: "Amux",
    providerKey: "cc-switch-amux",
    websiteUrl: "https://amux.ai",
    apiKeyUrl: "https://amux.ai",
    settingsConfig: {
      name: "Amux",
      baseUrl: "https://api.amux.ai/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.6-sol", {
          id: "gpt-5.6-sol",
        }),
      ],
    },
    category: "aggregator",
    icon: "amux",
  },
  {
    name: "DeepSeek",
    providerKey: "cc-switch-deep-seek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    settingsConfig: {
      name: "DeepSeek",
      baseUrl: "https://api.deepseek.com/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("deepseek/deepseek-v4-pro", {
          id: "deepseek-v4-pro",
          thinkingProfile: "deepseekV4",
        }),
        piModel("deepseek/deepseek-v4-flash", {
          id: "deepseek-v4-flash",
          thinkingProfile: "deepseekV4",
        }),
      ],
    },
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#1E88E5",
  },
  {
    name: "Zhipu GLM",
    providerKey: "cc-switch-zhipu-glm",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    settingsConfig: {
      name: "Zhipu GLM",
      baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("zai/glm-5.1", {
          id: "glm-5.1",
        }),
      ],
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
  },
  {
    name: "Zhipu GLM en",
    providerKey: "cc-switch-zhipu-glm-en",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe?ic=8JVLJQFSKB",
    settingsConfig: {
      name: "Zhipu GLM en",
      baseUrl: "https://api.z.ai/api/coding/paas/v4",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("zai/glm-5.1", {
          id: "glm-5.1",
        }),
      ],
    },
    category: "cn_official",
    icon: "zhipu",
    iconColor: "#0F62FE",
  },
  {
    name: "Bailian",
    providerKey: "cc-switch-bailian",
    websiteUrl: "https://bailian.console.aliyun.com",
    apiKeyUrl: "https://bailian.console.aliyun.com/#/api-key",
    settingsConfig: {
      name: "Bailian",
      baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("qwen/qwen3-coder-plus", {
          id: "qwen3-coder-plus",
        }),
      ],
    },
    category: "cn_official",
    icon: "bailian",
    iconColor: "#624AFF",
  },
  // ===== QwenCloud（DashScope 国际站）=====
  // 按量付费走 OpenAI 兼容层，Coding / Token Plan 走官方 Anthropic 地址
  // （与 Pi 其余 anthropic-messages 预设一致，地址不带 /v1）。
  {
    name: "QwenCloud",
    providerKey: "cc-switch-qwencloud",
    websiteUrl: "https://www.qwencloud.com",
    apiKeyUrl: "https://home.qwencloud.com/api-keys",
    settingsConfig: {
      name: "QwenCloud",
      baseUrl: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("qwen/qwen3.7-max", { id: "qwen3.7-max" }),
        piModel("qwen/qwen3.7-plus", { id: "qwen3.7-plus" }),
      ],
    },
    category: "cn_official",
    icon: "qwen",
    iconColor: "#6336E7",
  },
  {
    name: "QwenCloud For Coding",
    providerKey: "cc-switch-qwencloud-coding",
    websiteUrl: "https://www.qwencloud.com",
    apiKeyUrl: "https://home.qwencloud.com/api-keys",
    settingsConfig: {
      name: "QwenCloud For Coding",
      baseUrl: "https://coding-intl.dashscope.aliyuncs.com/apps/anthropic",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("qwen/qwen3.7-plus", { id: "qwen3.7-plus" }),
        piModel("qwen/qwen3-coder-plus", {
          id: "qwen3-coder-plus",
          contextWindow: 131_072,
        }),
      ],
    },
    category: "cn_official",
    icon: "qwen",
    iconColor: "#6336E7",
  },
  {
    name: "QwenCloud Token Plan",
    providerKey: "cc-switch-qwencloud-token-plan",
    websiteUrl: "https://www.qwencloud.com",
    apiKeyUrl: "https://home.qwencloud.com/api-keys",
    settingsConfig: {
      name: "QwenCloud Token Plan",
      baseUrl:
        "https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("qwen/qwen3.8-max", { id: "qwen3.8-max" }),
        piModel("qwen/qwen3.7-max", { id: "qwen3.7-max" }),
      ],
    },
    category: "cn_official",
    icon: "qwen",
    iconColor: "#6336E7",
  },
  {
    name: "StepFun",
    providerKey: "cc-switch-step-fun",
    websiteUrl: "https://platform.stepfun.com/step-plan",
    apiKeyUrl: "https://platform.stepfun.com/interface-key",
    settingsConfig: {
      name: "StepFun",
      baseUrl: "https://api.stepfun.com/step_plan/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("stepfun/step-3.5-flash", {
          id: "step-3.5-flash-2603",
          name: "Step 3.5 Flash 2603",
        }),
        piModel("stepfun/step-3.5-flash", {
          id: "step-3.5-flash",
        }),
      ],
    },
    category: "cn_official",
    icon: "stepfun",
    iconColor: "#16D6D2",
  },
  {
    name: "StepFun en",
    providerKey: "cc-switch-step-fun-en",
    websiteUrl: "https://platform.stepfun.ai/step-plan",
    apiKeyUrl: "https://platform.stepfun.ai/interface-key",
    settingsConfig: {
      name: "StepFun en",
      baseUrl: "https://api.stepfun.ai/step_plan/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("stepfun/step-3.5-flash", {
          id: "step-3.5-flash-2603",
          name: "Step 3.5 Flash 2603",
        }),
        piModel("stepfun/step-3.5-flash", {
          id: "step-3.5-flash",
        }),
      ],
    },
    category: "cn_official",
    icon: "stepfun",
    iconColor: "#16D6D2",
  },
  {
    name: "StepFun Step Plan",
    providerKey: "cc-switch-step-fun-step-plan",
    websiteUrl: "https://platform.stepfun.com/docs/zh/step-plan/overview",
    apiKeyUrl: "https://platform.stepfun.com/interface-key",
    settingsConfig: {
      name: "StepFun Step Plan",
      baseUrl: "https://api.stepfun.com/step_plan/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("stepfun/step-3.5-flash", {
          id: "step-3.5-flash",
        }),
      ],
    },
    category: "cn_official",
    icon: "stepfun",
    iconColor: "#005AFF",
  },
  {
    name: "ModelScope",
    providerKey: "cc-switch-model-scope",
    websiteUrl: "https://modelscope.cn",
    apiKeyUrl: "https://modelscope.cn/my/myaccesstoken",
    settingsConfig: {
      name: "ModelScope",
      baseUrl: "https://api-inference.modelscope.cn/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("zai/glm-5.2", {
          id: "ZhipuAI/GLM-5.2",
        }),
      ],
    },
    category: "aggregator",
    icon: "modelscope",
    iconColor: "#624AFF",
  },
  {
    name: "KAT-Coder",
    providerKey: "cc-switch-kat-coder",
    websiteUrl: "https://console.streamlake.ai",
    apiKeyUrl: "https://console.streamlake.ai/console/api-key",
    settingsConfig: {
      name: "KAT-Coder",
      baseUrl:
        "https://vanchin.streamlake.ai/api/gateway/v1/endpoints/${ENDPOINT_ID}/openai",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("streamlake/kat-coder-pro", {
          id: "KAT-Coder-Pro",
        }),
      ],
    },
    category: "cn_official",
    icon: "catcoder",
  },
  {
    name: "Longcat",
    providerKey: "cc-switch-longcat",
    websiteUrl: "https://longcat.chat/platform",
    apiKeyUrl: "https://longcat.chat/platform/api_keys",
    settingsConfig: {
      name: "Longcat",
      baseUrl: "https://api.longcat.chat/openai/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("longcat/longcat-2.0", {
          id: "LongCat-2.0",
        }),
      ],
    },
    category: "cn_official",
    icon: "longcat",
    iconColor: "#29E154",
  },
  {
    name: "MiniMax",
    providerKey: "cc-switch-mini-max",
    websiteUrl: "https://platform.minimaxi.com",
    apiKeyUrl: "https://platform.minimaxi.com/subscribe/coding-plan",
    settingsConfig: {
      name: "MiniMax",
      baseUrl: "https://api.minimaxi.com/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("minimax/minimax-m2.7", {
          id: "MiniMax-M2.7",
        }),
      ],
    },
    category: "cn_official",
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
    providerKey: "cc-switch-mini-max-en",
    websiteUrl: "https://platform.minimax.io",
    apiKeyUrl: "https://platform.minimax.io/subscribe/coding-plan",
    settingsConfig: {
      name: "MiniMax en",
      baseUrl: "https://api.minimax.io/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("minimax/minimax-m2.7", {
          id: "MiniMax-M2.7",
        }),
      ],
    },
    category: "cn_official",
    partnerPromotionKey: "minimax_en",
    theme: {
      backgroundColor: "#f64551",
      textColor: "#FFFFFF",
    },
    icon: "minimax",
    iconColor: "#FF6B6B",
  },
  {
    name: "BaiLing",
    providerKey: "cc-switch-bai-ling",
    websiteUrl: "https://alipaytbox.yuque.com/sxs0ba/ling/get_started",
    settingsConfig: {
      name: "BaiLing",
      baseUrl: "https://api.tbox.cn/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("inclusionai/ling-2.5-1t", {
          id: "Ling-2.5-1T",
        }),
      ],
    },
    category: "cn_official",
  },
  {
    name: "Xiaomi MiMo",
    providerKey: "cc-switch-xiaomi-mi-mo",
    websiteUrl: "https://platform.xiaomimimo.com",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/api-keys",
    settingsConfig: {
      name: "Xiaomi MiMo",
      baseUrl: "https://api.xiaomimimo.com/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        {
          ...piModel("xiaomi/mimo-v2.5-pro", {
            id: "mimo-v2.5-pro",
          }),
          compat: { ...XIAOMI_THINKING_COMPAT },
        },
        {
          ...piModel("xiaomi/mimo-v2.5", {
            id: "mimo-v2.5",
          }),
          compat: { ...XIAOMI_THINKING_COMPAT },
        },
      ],
    },
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
  },
  {
    name: "Xiaomi MiMo Token Plan (China)",
    providerKey: "cc-switch-xiaomi-mi-mo-token-plan-china",
    websiteUrl: "https://platform.xiaomimimo.com/#/token-plan",
    apiKeyUrl: "https://platform.xiaomimimo.com/#/console/plan-manage",
    settingsConfig: {
      name: "Xiaomi MiMo Token Plan (China)",
      baseUrl: "https://token-plan-cn.xiaomimimo.com/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("xiaomi/mimo-v2.5-pro", {
          id: "mimo-v2.5-pro",
        }),
        piModel("xiaomi/mimo-v2.5", {
          id: "mimo-v2.5",
        }),
      ],
    },
    category: "cn_official",
    icon: "xiaomimimo",
    iconColor: "#000000",
  },
  {
    name: "OpenCode Go",
    providerKey: "cc-switch-open-code-go",
    websiteUrl: "https://opencode.ai/go",
    apiKeyUrl: "https://opencode.ai/go?ref=2YTRG2NGTX",
    settingsConfig: {
      name: "OpenCode Go",
      baseUrl: "https://opencode.ai/zen/go/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        {
          ...piModel("zai/glm-5.2", {
            id: "glm-5.2",
            name: "GLM 5.2",
            thinkingProfile: "openCodeGoGlm52",
          }),
          compat: { ...OPENAI_COMPLETIONS_COMPAT },
        },
        {
          ...piModel("moonshotai/kimi-k2.7-code", {
            id: "kimi-k2.7-code",
          }),
          compat: { ...OPENAI_COMPLETIONS_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("xiaomi/mimo-v2.5-pro", {
            id: "mimo-v2.5-pro",
          }),
          compat: { ...OPENAI_COMPLETIONS_COMPAT },
        },
      ],
    },
    category: "third_party",
    partnerPromotionKey: "opencode_go",
    icon: "opencode",
    iconColor: "#211E1E",
  },
  {
    name: "AiHubMix",
    providerKey: "cc-switch-ai-hub-mix",
    websiteUrl: "https://aihubmix.com",
    apiKeyUrl: "https://aihubmix.com",
    settingsConfig: {
      name: "AiHubMix",
      baseUrl: "https://aihubmix.com",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
        }),
      ],
    },
    category: "aggregator",
    icon: "aihubmix",
    iconColor: "#006FFB",
  },
  {
    name: "CherryIN",
    providerKey: "cc-switch-cherry-in",
    websiteUrl: "https://open.cherryin.ai",
    apiKeyUrl: "https://open.cherryin.ai/console/token",
    settingsConfig: {
      name: "CherryIN",
      baseUrl: "https://open.cherryin.net",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-sonnet-5", {
          id: "anthropic/claude-sonnet-5",
        }),
        piModel("anthropic/claude-opus-5", {
          id: "anthropic/claude-opus-5",
        }),
      ],
    },
    category: "aggregator",
    icon: "cherryin",
  },
  {
    name: "OpenRouter",
    providerKey: "cc-switch-open-router",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      name: "OpenRouter",
      baseUrl: "https://openrouter.ai/api",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-sonnet-5", {
          id: "anthropic/claude-sonnet-5",
        }),
        piModel("anthropic/claude-opus-5", {
          id: "anthropic/claude-opus-5",
        }),
      ],
    },
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6566F1",
  },
  {
    name: "TheRouter",
    providerKey: "cc-switch-the-router",
    websiteUrl: "https://therouter.ai",
    apiKeyUrl: "https://dashboard.therouter.ai",
    settingsConfig: {
      name: "TheRouter",
      baseUrl: "https://api.therouter.ai/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("anthropic/claude-sonnet-5", {
          id: "anthropic/claude-sonnet-5",
        }),
        piModel("openai/gpt-5.3-codex", {
          id: "openai/gpt-5.3-codex",
        }),
        piModel("openai/gpt-5.2", {
          id: "openai/gpt-5.2",
        }),
        piModel("google/gemini-3.6-flash", {
          id: "google/gemini-3.6-flash",
        }),
        piModel("qwen/qwen3-coder-480b", {
          id: "qwen/qwen3-coder-480b",
        }),
      ],
    },
    category: "aggregator",
  },
  {
    name: "Novita AI",
    providerKey: "cc-switch-novita-ai",
    websiteUrl: "https://novita.ai",
    apiKeyUrl: "https://novita.ai",
    settingsConfig: {
      name: "Novita AI",
      baseUrl: "https://api.novita.ai/openai",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("zai/glm-5.1", {
          id: "zai-org/glm-5.1",
        }),
      ],
    },
    category: "aggregator",
    icon: "novita",
    iconColor: "#000000",
  },
  {
    name: "Nvidia",
    providerKey: "cc-switch-nvidia",
    websiteUrl: "https://build.nvidia.com",
    apiKeyUrl: "https://build.nvidia.com/settings/api-keys",
    settingsConfig: {
      name: "Nvidia",
      baseUrl: "https://integrate.api.nvidia.com/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("moonshotai/kimi-k2.5", {
          id: "moonshotai/kimi-k2.5",
        }),
      ],
    },
    category: "aggregator",
    icon: "nvidia",
    iconColor: "#000000",
  },
  {
    name: "PIPELLM",
    providerKey: "cc-switch-pipellm",
    websiteUrl: "https://code.pipellm.ai",
    apiKeyUrl: "https://code.pipellm.ai/login?ref=uvw650za",
    settingsConfig: {
      name: "PIPELLM",
      baseUrl: "https://cc-api.pipellm.ai",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        piModel("anthropic/claude-opus-5", {
          id: "claude-opus-5",
          name: "claude-opus-5",
        }),
        piModel("anthropic/claude-sonnet-5", {
          id: "claude-sonnet-5",
          name: "claude-sonnet-5",
        }),
        piModel("anthropic/claude-haiku-4.5-20251001", {
          id: "claude-haiku-4-5-20251001",
          name: "claude-haiku-4-5-20251001",
        }),
      ],
    },
    category: "aggregator",
    icon: "pipellm",
  },
  {
    name: "AICodeWith",
    providerKey: "cc-switch-aicode-with",
    websiteUrl: "https://aicodewith.ai",
    apiKeyUrl: "https://aicodewith.ai/login?tab=register",
    settingsConfig: {
      name: "AICodeWith",
      baseUrl: "https://api.aicodewith.ai/chatgpt/v1",
      api: "openai-responses",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.6-sol", {
          id: "gpt-5.6-sol",
          name: "gpt-5.6-sol",
        }),
      ],
    },
    category: "aggregator",
    icon: "aicodewith",
    iconColor: "#3A3B40",
  },
  {
    name: "E-FlowCode",
    providerKey: "cc-switch-e-flow-code",
    websiteUrl: "https://e-flowcode.cc",
    apiKeyUrl: "https://e-flowcode.cc",
    settingsConfig: {
      name: "E-FlowCode",
      baseUrl: "https://e-flowcode.cc/v1",
      api: "openai-responses",
      apiKey: "",
      models: [
        piModel("openai/gpt-5.2-codex", {
          id: "gpt-5.2-codex",
          name: "gpt-5.2-codex",
        }),
        piModel("openai/gpt-5.3-codex", {
          id: "gpt-5.3-codex",
          name: "gpt-5.3-codex",
        }),
      ],
    },
    category: "third_party",
    icon: "eflowcode",
    iconColor: "#000000",
  },
  {
    name: "AWS Bedrock",
    providerKey: "cc-switch-aws-bedrock",
    websiteUrl: "https://aws.amazon.com/bedrock/",
    settingsConfig: {
      name: "AWS Bedrock",
      baseUrl: "https://bedrock-runtime.us-east-1.amazonaws.com",
      api: "bedrock-converse-stream",
      apiKey: "",
      models: [
        piModel("anthropic/claude-opus-5", {
          id: "global.anthropic.claude-opus-5",
          thinkingProfile: "xhighAndMax",
        }),
        piModel("anthropic/claude-sonnet-5", {
          id: "global.anthropic.claude-sonnet-5",
          thinkingProfile: "xhighAndMax",
        }),
        piModel("anthropic/claude-haiku-4.5-20251001", {
          id: "global.anthropic.claude-haiku-4-5-20251001-v1:0",
        }),
        piModel("amazon/nova-pro", {
          id: "us.amazon.nova-pro-v1:0",
        }),
        piModel("meta/llama-4-maverick", {
          id: "us.meta.llama4-maverick-17b-instruct-v1:0",
        }),
        piModel("deepseek/deepseek-r1", {
          id: "us.deepseek.r1-v1:0",
        }),
      ],
    },
    category: "cloud_provider",
    icon: "aws",
    iconColor: "#FF9900",
  },
  // ===== 腾讯云 Token Plan（订阅套餐，产品线 1823/1300）=====
  // 与 claude/codex/opencode/hermes/openclaw 六 app 的腾讯预设同源：
  // 个人版国内走 api.lkeap.cloud.tencent.com/plan，其余走 tencentmaas.com/plan
  // 域族；两站 Key 不互通，双地域候选（国内→新加坡、国际→广州）仅在企业
  // 套餐存在。Pi 预设只携带默认地域 baseUrl（PiProviderPreset 无
  // endpointCandidates 字段），需要第二地域的用户在 UI 中手动改 baseUrl。
  {
    name: "Tencent Token Plan",
    providerKey: "cc-switch-tencent-token-plan",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan",
    settingsConfig: {
      name: "Tencent Token Plan",
      baseUrl: "https://api.lkeap.cloud.tencent.com/plan/v3",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("tencent/tokenplan-auto", { id: "tc-code-latest" }),
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash-202605",
            name: "DeepSeek V4 Flash Official",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro-202606",
            name: "DeepSeek V4 Pro Official",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        piModel("minimax/minimax-m2.7", { id: "minimax-m2.7" }),
        piModel("zai/glm-5", { id: "glm-5" }),
        piModel("zai/glm-5.1", { id: "glm-5.1" }),
        piModel("zai/glm-5.2", { id: "glm-5.2" }),
        piModel("tencent/hy3", { id: "hy3" }),
        piModel("tencent/hy3-preview", { id: "hy3-preview" }),
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#00A4FF",
  },
  {
    name: "Tencent Token Plan (Intl)",
    providerKey: "cc-switch-tencent-token-plan-intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan",
    settingsConfig: {
      name: "Tencent Token Plan (Intl)",
      baseUrl: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("tencent/tokenplan-auto", { id: "auto" }),
        piModel("zai/glm-5.2", { id: "glm-5.2" }),
        piModel("moonshotai/kimi-k2.6", { id: "kimi-k2.6" }),
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro-202606",
            name: "DeepSeek V4 Pro Official",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash-202605",
            name: "DeepSeek V4 Flash Official",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        piModel("minimax/minimax-m3", { id: "minimax-m3" }),
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#00A4FF",
  },
  {
    name: "Tencent Token Plan Enterprise Pro",
    providerKey: "cc-switch-tencent-token-plan-enterprise-pro",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan-e",
    settingsConfig: {
      name: "Tencent Token Plan Enterprise Pro",
      baseUrl: "https://tokenhub.tencentmaas.com/plan/v3",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("tencent/tokenplan-auto", { id: "auto" }),
        piModel("zai/glm-5.3", { id: "glm-5.3" }),
        piModel("zai/glm-5.2", { id: "glm-5.2" }),
        piModel("zai/glm-5", { id: "glm-5" }),
        piModel("zai/glm-5.1", { id: "glm-5.1" }),
        piModel("zai/glm-5-turbo", { id: "glm-5-turbo" }),
        piModel("moonshotai/kimi-k2.7-code", {
          id: "kimi-k2.7-code",
          thinkingProfile: "offUnsupported",
        }),
        piModel("moonshotai/kimi-k2.7-code-highspeed", {
          id: "kimi-k2.7-code-highspeed",
          thinkingProfile: "offUnsupported",
        }),
        piModel("moonshotai/kimi-k2.6", { id: "kimi-k2.6" }),
        piModel("minimax/minimax-m2.7", { id: "minimax-m2.7" }),
        piModel("minimax/minimax-m3", { id: "minimax-m3" }),
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash-0731",
            name: "DeepSeek V4 Flash 0731 GA",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro-0813",
            name: "DeepSeek V4 Pro 0813 GA",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash-202605",
            name: "DeepSeek V4 Flash Official",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro-202606",
            name: "DeepSeek V4 Pro Official",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#00A4FF",
  },
  {
    name: "Tencent Token Plan Enterprise Pro (Intl)",
    providerKey: "cc-switch-tencent-token-plan-enterprise-pro-intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan-e",
    settingsConfig: {
      name: "Tencent Token Plan Enterprise Pro (Intl)",
      baseUrl: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("tencent/tokenplan-auto", { id: "auto" }),
        piModel("zai/glm-5.3", { id: "glm-5.3" }),
        piModel("zai/glm-5.2", { id: "glm-5.2" }),
        piModel("minimax/minimax-m3", { id: "minimax-m3" }),
        piModel("moonshotai/kimi-k2.7-code", {
          id: "kimi-k2.7-code",
          thinkingProfile: "offUnsupported",
        }),
        piModel("moonshotai/kimi-k2.7-code-highspeed", {
          id: "kimi-k2.7-code-highspeed",
          thinkingProfile: "offUnsupported",
        }),
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash-0731",
            name: "DeepSeek V4 Flash 0731 GA",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro-0813",
            name: "DeepSeek V4 Pro 0813 GA",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash-202605",
            name: "DeepSeek V4 Flash Official",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro-202606",
            name: "DeepSeek V4 Pro Official",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#00A4FF",
  },
  {
    name: "Tencent Token Plan Enterprise Lite",
    providerKey: "cc-switch-tencent-token-plan-enterprise-lite",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/tokenplan-e",
    settingsConfig: {
      name: "Tencent Token Plan Enterprise Lite",
      baseUrl: "https://tokenhub.tencentmaas.com/plan/v3",
      api: "openai-completions",
      apiKey: "",
      models: [piModel("tencent/tokenplan-auto", { id: "auto" })],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#00A4FF",
  },
  {
    name: "Tencent Token Plan Enterprise Lite (Intl)",
    providerKey: "cc-switch-tencent-token-plan-enterprise-lite-intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/tokenplan-e",
    settingsConfig: {
      name: "Tencent Token Plan Enterprise Lite (Intl)",
      baseUrl: "https://tokenhub-intl.tencentcloudmaas.com/plan/v3",
      api: "openai-completions",
      apiKey: "",
      models: [piModel("tencent/tokenplan-auto", { id: "auto" })],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#00A4FF",
  },
  // ===== 腾讯云 TokenHub（按量付费 API 市场，产品线 1823/1300，/v1 端点）=====
  // 与 Token Plan 订阅线（/plan 端点）是两条独立产品线：Key 不互通、端点不同。
  // 模型清单取自官方「语言模型」列表（国内 130051 / 国际 78934），只收录支持
  // Function Calling 的通用语言模型；翻译（hy-mt2）、角色扮演（hy-role）等
  // 专用模型不收录（无法用于编码 agent）。
  {
    name: "Tencent TokenHub",
    providerKey: "cc-switch-tencent-tokenhub",
    websiteUrl: "https://cloud.tencent.com/product/tokenhub",
    apiKeyUrl: "https://console.cloud.tencent.com/tokenhub/apikey",
    settingsConfig: {
      name: "Tencent TokenHub",
      baseUrl: "https://tokenhub.tencentmaas.com/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("tencent/hy4-preview", { id: "hy4-preview" }),
        piModel("tencent/hy3", { id: "hy3" }),
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash-202605",
            name: "DeepSeek V4 Flash Official",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro-202606",
            name: "DeepSeek V4 Pro Official",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-flash-vision-exp", {
            id: "deepseek/deepseek-v4-flash-vision-exp",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash-0731",
            name: "DeepSeek V4 Flash 0731 GA",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro-0813",
            name: "DeepSeek V4 Pro 0813 GA",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        piModel("zai/glm-5.3-flash", { id: "glm-5.3-flash" }),
        piModel("zai/glm-5.3", { id: "glm-5.3" }),
        piModel("zai/glm-5.2", { id: "glm-5.2" }),
        piModel("zai/glm-5.1", { id: "glm-5.1" }),
        piModel("zai/glm-5v-turbo", { id: "glm-5v-turbo" }),
        piModel("zai/glm-5-turbo", { id: "glm-5-turbo" }),
        piModel("zai/glm-5", { id: "glm-5" }),
        piModel("moonshotai/kimi-k2.7-code-highspeed", {
          id: "kimi-k2.7-code-highspeed",
          thinkingProfile: "offUnsupported",
        }),
        piModel("moonshotai/kimi-k3", { id: "kimi-k3" }),
        piModel("moonshotai/kimi-k2.7-code", {
          id: "kimi-k2.7-code",
          thinkingProfile: "offUnsupported",
        }),
        piModel("moonshotai/kimi-k2.6", { id: "kimi-k2.6" }),
        piModel("moonshotai/kimi-k2.5", { id: "kimi-k2.5" }),
        piModel("minimax/minimax-m3", { id: "minimax-m3" }),
        piModel("minimax/minimax-m2.7", { id: "minimax-m2.7" }),
        piModel("xiaomi/mimo-v2.5-pro", { id: "mimo-v2.5-pro" }),
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#00A4FF",
  },
  {
    name: "Tencent TokenHub (Intl)",
    providerKey: "cc-switch-tencent-tokenhub-intl",
    websiteUrl: "https://www.tencentcloud.com/products/tokenhub",
    apiKeyUrl: "https://console.tencentcloud.com/tokenhub/apikey",
    settingsConfig: {
      name: "Tencent TokenHub (Intl)",
      baseUrl: "https://tokenhub-intl.tencentcloudmaas.com/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        piModel("tencent/hy4-preview", { id: "hy4-preview" }),
        piModel("tencent/hy3", { id: "hy3" }),
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash-202605",
            name: "DeepSeek V4 Flash Official",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro-202606",
            name: "DeepSeek V4 Pro Official",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-flash-vision-exp", {
            id: "deepseek/deepseek-v4-flash-vision-exp",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash-0731",
            name: "DeepSeek V4 Flash 0731 GA",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro-0813",
            name: "DeepSeek V4 Pro 0813 GA",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-flash", {
            id: "deepseek-v4-flash",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v4-pro", {
            id: "deepseek-v4-pro",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        {
          ...piModel("deepseek/deepseek-v3.2", {
            id: "deepseek-v3.2",
            thinkingProfile: "deepseekV4",
          }),
          compat: { ...TENCENT_DEEPSEEK_THINKING_COMPAT },
        },
        piModel("zai/glm-5.3", { id: "glm-5.3" }),
        piModel("zai/glm-5.3-flash", { id: "glm-5.3-flash" }),
        piModel("zai/glm-5.2", { id: "glm-5.2" }),
        piModel("zai/glm-5", { id: "glm-5" }),
        piModel("zai/glm-5-turbo", { id: "glm-5-turbo" }),
        piModel("zai/glm-5v-turbo", { id: "glm-5v-turbo" }),
        piModel("zai/glm-5.1", { id: "glm-5.1" }),
        piModel("moonshotai/kimi-k3", { id: "kimi-k3" }),
        piModel("moonshotai/kimi-k2.7-code-highspeed", {
          id: "kimi-k2.7-code-highspeed",
          thinkingProfile: "offUnsupported",
        }),
        piModel("moonshotai/kimi-k2.7-code", {
          id: "kimi-k2.7-code",
          thinkingProfile: "offUnsupported",
        }),
        piModel("moonshotai/kimi-k2.6", { id: "kimi-k2.6" }),
        piModel("moonshotai/kimi-k2.5", { id: "kimi-k2.5" }),
        piModel("minimax/minimax-m3", { id: "minimax-m3" }),
        piModel("minimax/minimax-m2.7", { id: "minimax-m2.7" }),
        piModel("xiaomi/mimo-v2.5-pro", { id: "mimo-v2.5-pro" }),
      ],
    },
    category: "cn_official",
    icon: "tencent",
    iconColor: "#00A4FF",
  },
];

function materializeVerifiedThinkingProfiles(
  preset: PiProviderPreset,
): PiProviderPreset {
  return {
    ...preset,
    settingsConfig: {
      ...preset.settingsConfig,
      models: preset.settingsConfig.models.map((model) => {
        const reference = getPiModelCatalogReference(model);
        if (!reference) return model;
        const resolved = reference.presetThinkingProfileId
          ? getPiThinkingProfile(reference.presetThinkingProfileId)
          : resolvePiThinkingProfile({
              catalogKey: reference.catalogKey,
              api: preset.settingsConfig.api,
            });
        return {
          ...model,
          ...(model.reasoning ? { thinkingLevelMap: resolved?.map ?? {} } : {}),
          ...(resolved?.modelCompat
            ? {
                compat: {
                  ...model.compat,
                  ...resolved.modelCompat,
                },
              }
            : {}),
        };
      }),
    },
  };
}

export const piProviderPresets = piProviderPresetDefinitions.map(
  materializeVerifiedThinkingProfiles,
);
