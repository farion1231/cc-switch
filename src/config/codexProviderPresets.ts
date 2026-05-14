/**
 * Codex 预设供应商配置模板
 */
import { ProviderCategory } from "../types";
import type { PresetTheme } from "./claudeProviderPresets";

export interface CodexProviderPreset {
  name: string;
  nameKey?: string; // i18n key for localized display name
  websiteUrl: string;
  // 第三方供应商可提供单独的获取 API Key 链接
  apiKeyUrl?: string;
  auth: Record<string, any>; // 将写入 ~/.codex/auth.json
  config: string; // 将写入 ~/.codex/config.toml（TOML 字符串）
  isOfficial?: boolean; // 标识是否为官方预设
  isPartner?: boolean; // 标识是否为商业合作伙伴
  partnerPromotionKey?: string; // 合作伙伴促销信息的 i18n key
  category?: ProviderCategory; // 新增：分类
  isCustomTemplate?: boolean; // 标识是否为自定义模板
  // 新增：请求地址候选列表（用于地址管理/测速）
  endpointCandidates?: string[];
  // 新增：视觉主题配置
  theme?: PresetTheme;
  // 图标配置
  icon?: string; // 图标名称
  iconColor?: string; // 图标颜色
}

/**
 * 生成第三方供应商的 auth.json
 */
export function generateThirdPartyAuth(apiKey: string): Record<string, any> {
  return {
    OPENAI_API_KEY: apiKey || "",
  };
}

/**
 * 生成第三方供应商的 config.toml
 */
export function generateThirdPartyConfig(
  providerName: string,
  baseUrl: string,
  modelName = "gpt-5.4",
): string {
  // 清理供应商名称，确保符合TOML键名规范
  const cleanProviderName =
    providerName
      .toLowerCase()
      .replace(/[^a-z0-9_]/g, "_")
      .replace(/^_+|_+$/g, "") || "custom";

  return `model_provider = "${cleanProviderName}"
model = "${modelName}"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.${cleanProviderName}]
name = "${cleanProviderName}"
base_url = "${baseUrl}"
wire_api = "responses"
requires_openai_auth = true`;
}

export const codexProviderPresets: CodexProviderPreset[] = [
  {
    name: "OpenAI Official",
    websiteUrl: "https://chatgpt.com/codex",
    isOfficial: true,
    category: "official",
    auth: {},
    config: ``,
    theme: {
      icon: "codex",
      backgroundColor: "#1F2937", // gray-800
      textColor: "#FFFFFF",
    },
    icon: "openai",
    iconColor: "#00A67E",
  },
  {
    name: "Shengsuanyun",
    nameKey: "providerForm.presets.shengsuanyun",
    websiteUrl: "https://www.shengsuanyun.com",
    apiKeyUrl: "https://www.shengsuanyun.com/?from=CH_4HHXMRYF",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "shengsuanyun",
      "https://router.shengsuanyun.com/api/v1",
      "gpt-5.4",
    ),
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "shengsuanyun",
    icon: "shengsuanyun",
  },
  {
    name: "Azure OpenAI",
    websiteUrl:
      "https://learn.microsoft.com/en-us/azure/ai-foundry/openai/how-to/codex",
    category: "third_party",
    isOfficial: true,
    auth: generateThirdPartyAuth(""),
    config: `model_provider = "azure"
model = "gpt-5.4"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.azure]
name = "Azure OpenAI"
base_url = "https://YOUR_RESOURCE_NAME.openai.azure.com/openai"
env_key = "OPENAI_API_KEY"
query_params = { "api-version" = "2025-04-01-preview" }
wire_api = "responses"
requires_openai_auth = true`,
    endpointCandidates: ["https://YOUR_RESOURCE_NAME.openai.azure.com/openai"],
    theme: {
      icon: "codex",
      backgroundColor: "#0078D4",
      textColor: "#FFFFFF",
    },
    icon: "azure",
    iconColor: "#0078D4",
  },
  {
    name: "AiHubMix",
    websiteUrl: "https://aihubmix.com",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "aihubmix",
      "https://aihubmix.com/v1",
      "gpt-5.4",
    ),
    endpointCandidates: [
      "https://aihubmix.com/v1",
      "https://api.aihubmix.com/v1",
    ],
  },
  {
    name: "DMXAPI",
    websiteUrl: "https://www.dmxapi.cn",
    category: "aggregator",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "dmxapi",
      "https://www.dmxapi.cn/v1",
      "gpt-5.4",
    ),
    endpointCandidates: ["https://www.dmxapi.cn/v1"],
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "dmxapi", // 促销信息 i18n key
  },
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.com",
    apiKeyUrl: "https://www.packyapi.com/register?aff=cc-switch",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "packycode",
      "https://www.packyapi.com/v1",
      "gpt-5.4",
    ),
    endpointCandidates: [
      "https://www.packyapi.com/v1",
      "https://api-slb.packyapi.com/v1",
    ],
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "packycode", // 促销信息 i18n key
    icon: "packycode",
  },
  {
    name: "Cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "cubence",
      "https://api.cubence.com/v1",
      "gpt-5.4",
    ),
    endpointCandidates: [
      "https://api.cubence.com/v1",
      "https://api-cf.cubence.com/v1",
      "https://api-dmit.cubence.com/v1",
      "https://api-bwg.cubence.com/v1",
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
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "aigocode",
      "https://api.aigocode.com",
      "gpt-5.4",
    ),
    endpointCandidates: ["https://api.aigocode.com"],
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "aigocode", // 促销信息 i18n key
    icon: "aigocode",
    iconColor: "#5B7FFF",
  },
  {
    name: "RightCode",
    websiteUrl: "https://www.right.codes",
    apiKeyUrl: "https://www.right.codes/register?aff=CCSWITCH",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "rightcode",
      "https://right.codes/codex/v1",
      "gpt-5.4",
    ),
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
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "aicodemirror",
      "https://api.aicodemirror.com/api/codex/backend-api/codex",
      "gpt-5.4",
    ),
    endpointCandidates: [
      "https://api.aicodemirror.com/api/codex/backend-api/codex",
      "https://api.claudecode.net.cn/api/codex/backend-api/codex",
    ],
    isPartner: true,
    partnerPromotionKey: "aicodemirror",
    icon: "aicodemirror",
    iconColor: "#000000",
  },
  {
    name: "AICoding",
    websiteUrl: "https://aicoding.sh",
    apiKeyUrl: "https://aicoding.sh/i/CCSWITCH",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "aicoding",
      "https://api.aicoding.sh",
      "gpt-5.4",
    ),
    endpointCandidates: ["https://api.aicoding.sh"],
    isPartner: true,
    partnerPromotionKey: "aicoding",
    icon: "aicoding",
    iconColor: "#000000",
  },
  {
    name: "CrazyRouter",
    websiteUrl: "https://www.crazyrouter.com",
    apiKeyUrl: "https://www.crazyrouter.com/register?aff=OZcm&ref=cc-switch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "crazyrouter",
      "https://cn.crazyrouter.com/v1",
      "gpt-5.4",
    ),
    endpointCandidates: ["https://cn.crazyrouter.com/v1"],
    isPartner: true,
    partnerPromotionKey: "crazyrouter",
    icon: "crazyrouter",
    iconColor: "#000000",
  },
  {
    name: "SSSAiCode",
    websiteUrl: "https://www.sssaicode.com",
    apiKeyUrl: "https://www.sssaicode.com/register?ref=DCP0SM",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "sssaicode",
      "https://node-hk.sssaicode.com/api/v1",
      "gpt-5.4",
    ),
    endpointCandidates: [
      "https://node-hk.sssaicode.com/api/v1",
      "https://claude2.sssaicode.com/api/v1",
      "https://anti.sssaicode.com/api/v1",
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
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "compshare",
      "https://api.modelverse.cn/v1",
      "gpt-5.4",
    ),
    endpointCandidates: ["https://api.modelverse.cn/v1"],
    category: "aggregator",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "ucloud", // 促销信息 i18n key
    icon: "ucloud",
    iconColor: "#000000",
  },
  {
    name: "Compshare Coding Plan",
    nameKey: "providerForm.presets.ucloudCoding",
    websiteUrl: "https://www.compshare.cn",
    apiKeyUrl:
      "https://www.compshare.cn/coding-plan?ytag=GPU_YY_YX_git_cc-switch",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "compshare_coding",
      "https://cp.compshare.cn/v1",
      "gpt-5.4",
    ),
    endpointCandidates: ["https://cp.compshare.cn/v1"],
    category: "aggregator",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "ucloud", // 促销信息 i18n key（复用）
    icon: "ucloud",
    iconColor: "#000000",
  },
  {
    name: "Micu",
    websiteUrl: "https://www.micuapi.ai",
    apiKeyUrl: "https://www.micuapi.ai/register?aff=aOYQ",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "micu",
      "https://www.micuapi.ai/v1",
      "gpt-5.4",
    ),
    endpointCandidates: ["https://www.micuapi.ai/v1"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "micu", // 促销信息 i18n key
    icon: "micu",
    iconColor: "#000000",
  },
  {
    name: "CTok.ai",
    websiteUrl: "https://ctok.ai",
    apiKeyUrl: "https://ctok.ai",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "ctok",
      "https://api.ctok.ai/v1",
      "gpt-5.4",
    ),
    endpointCandidates: ["https://api.ctok.ai/v1"],
    category: "third_party",
    isPartner: true, // 合作伙伴
    partnerPromotionKey: "ctok", // 促销信息 i18n key
    icon: "ctok",
    iconColor: "#000000",
  },
  {
    name: "LionCCAPI",
    websiteUrl: "https://vibecodingapi.ai",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "lionccapi",
      "https://vibecodingapi.ai/v1",
      "gpt-5.4",
    ),
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "lionccapi",
    icon: "lioncc",
  },
  {
    name: "E-FlowCode",
    websiteUrl: "https://e-flowcode.cc",
    apiKeyUrl: "https://e-flowcode.cc",
    auth: {
      OPENAI_API_KEY: "",
    },
    config: `model_provider = "e-flowcode"
model = "gpt-5.4"
model_reasoning_effort = "high"
disable_response_storage = true
personality = "pragmatic"

[model_providers.e-flowcode]
name = "e-flowcode"
base_url = "https://e-flowcode.cc/v1"
wire_api = "responses"
requires_openai_auth = true
model_context_window = 1000000
model_auto_compact_token_limit = 9000000`,
    category: "third_party",
    endpointCandidates: ["https://e-flowcode.cc/v1"],
    icon: "eflowcode",
    iconColor: "#000000",
  },
  {
    name: "LemonData",
    websiteUrl: "https://lemondata.cc",
    apiKeyUrl: "https://lemondata.cc/r/FFX1ZDUP",
    category: "third_party",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "lemondata",
      "https://api.lemondata.cc/v1",
      "gpt-5.4",
    ),
    endpointCandidates: ["https://api.lemondata.cc/v1"],
    isPartner: true,
    partnerPromotionKey: "lemondata",
    icon: "lemondata",
  },
  {
    name: "PIPELLM",
    websiteUrl: "https://code.pipellm.ai",
    apiKeyUrl: "https://code.pipellm.ai/login?ref=uvw650za",
    auth: {
      OPENAI_API_KEY: "",
    },
    config: `model_provider = "custom"
model = "gpt-5.4"
model_reasoning_effort = "medium"
disable_response_storage = true

[model_providers.custom]
name = "custom"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://cc-api.pipellm.ai/v1"`,
    category: "aggregator",
    endpointCandidates: ["https://cc-api.pipellm.ai/v1"],
    icon: "pipellm",
  },
  {
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "openrouter",
      "https://openrouter.ai/api/v1",
      "gpt-5.4",
    ),
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6566F1",
  },
  {
    name: "TheRouter",
    websiteUrl: "https://therouter.ai",
    apiKeyUrl: "https://dashboard.therouter.ai",
    auth: generateThirdPartyAuth(""),
    config: generateThirdPartyConfig(
      "therouter",
      "https://api.therouter.ai/v1",
      "openai/gpt-5.3-codex",
    ),
    endpointCandidates: ["https://api.therouter.ai/v1"],
    category: "aggregator",
  },
];
