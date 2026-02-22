/**
 * IIAgent Provider Endpoints for CC Switch
 * 
 * Unified endpoint annotations that integrate with cc-switch's capability system.
 * Follows the exact same pattern as cc-switch's native endpoints.
 */

import type { ApiFormat, Transport, ProviderEndpoint } from "./slots";

const t = (
  formats: ApiFormat[],
  baseUrl: string,
  supportsModelsEndpoint = true,
): Transport => ({
  formats: new Set(formats),
  baseUrl,
  supportsModelsEndpoint,
});

export const IIAGENT_ENDPOINTS: ProviderEndpoint[] = [
  {
    id: "iiagent_anthropic",
    name: "IIAgent - Anthropic",
    category: "official",
    transport: t(["anthropic"], "https://api.anthropic.com", true),
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://github.com/ii-agent/ii-agent",
    apiKeyUrl: "https://console.anthropic.com/settings/keys",
  },
  {
    id: "iiagent_openai",
    name: "IIAgent - OpenAI",
    category: "official",
    transport: t(["openai_chat", "openai_responses"], "https://api.openai.com/v1", true),
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://github.com/ii-agent/ii-agent",
    apiKeyUrl: "https://platform.openai.com/api-keys",
  },
  {
    id: "iiagent_deepseek",
    name: "IIAgent - DeepSeek",
    category: "cn_official",
    transport: t(["anthropic", "openai_chat"], "https://api.deepseek.com", true),
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
  },
  {
    id: "iiagent_zhipu",
    name: "IIAgent - Zhipu GLM",
    category: "cn_official",
    transport: t(["anthropic", "openai_chat"], "https://open.bigmodel.cn/api/paas/v4", true),
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    isPartner: true,
    partnerPromotionKey: "zhipu",
  },
  {
    id: "iiagent_zhipu_en",
    name: "IIAgent - Zhipu GLM en",
    category: "cn_official",
    transport: t(["anthropic", "openai_chat"], "https://api.z.ai/v1", true),
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe?ic=8JVLJQFSKB",
    isPartner: true,
    partnerPromotionKey: "zhipu",
  },
  {
    id: "iiagent_bailian",
    name: "IIAgent - Bailian",
    category: "cn_official",
    transport: t(["anthropic", "openai_chat"], "https://dashscope.aliyuncs.com/compatible-mode/v1", true),
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://bailian.console.aliyun.com",
  },
  {
    id: "iiagent_kimi",
    name: "IIAgent - Kimi",
    category: "cn_official",
    transport: t(["anthropic", "openai_chat"], "https://api.moonshot.cn/v1", true),
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://platform.moonshot.cn/console",
    apiKeyUrl: "https://platform.moonshot.cn/console/api-keys",
  },
  {
    id: "iiagent_minimax",
    name: "IIAgent - MiniMax",
    category: "cn_official",
    transport: t(["anthropic", "openai_chat"], "https://api.minimaxi.com/v1", true),
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://platform.minimaxi.com",
    apiKeyUrl: "https://platform.minimaxi.com/subscribe/coding-plan",
    isPartner: true,
    partnerPromotionKey: "minimax_cn",
  },
  {
    id: "iiagent_minimax_en",
    name: "IIAgent - MiniMax en",
    category: "cn_official",
    transport: t(["anthropic", "openai_chat"], "https://api.minimax.io/v1", true),
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://api.minimax.io",
  },
  {
    id: "iiagent_custom_anthropic",
    name: "IIAgent - Custom Anthropic",
    category: "custom",
    transport: t(["anthropic", "openai_chat"], "", true),
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://github.com/ii-agent/ii-agent",
  },
  {
    id: "iiagent_custom_openai",
    name: "IIAgent - Custom OpenAI",
    category: "custom",
    transport: t(["openai_chat", "openai_responses"], "", true),
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://github.com/ii-agent/ii-agent",
  },
];

export default IIAGENT_ENDPOINTS;
