/**
 * IIAgent Provider Presets for CC Switch
 * 
 * Simple presets that work with cc-switch's automatic trait derivation.
 * The derivesApps() function automatically determines app compatibility from transport.formats.
 */

import type { ProviderCategory, ProviderMeta } from "../types";

export interface IIAgentProviderPreset {
  id: string;
  name: string;
  category: ProviderCategory;
  transport: {
    formats: ("anthropic" | "openai_chat" | "openai_responses" | "google")[];
    baseUrl: string;
    supportsModelsEndpoint: boolean;
  };
  icon: string;
  iconColor: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  meta?: ProviderMeta;
}

export const iiAgentPresets: IIAgentProviderPreset[] = [
  {
    id: "iiagent_anthropic",
    name: "IIAgent - Anthropic",
    category: "official",
    transport: {
      formats: ["anthropic"],
      baseUrl: "https://api.anthropic.com",
      supportsModelsEndpoint: true,
    },
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://github.com/ii-agent/ii-agent",
    apiKeyUrl: "https://console.anthropic.com/settings/keys",
    meta: {
      apiFormat: "anthropic",
      isNewApi: true,
    },
  },
  {
    id: "iiagent_openai",
    name: "IIAgent - OpenAI",
    category: "official",
    transport: {
      formats: ["openai_chat", "openai_responses"],
      baseUrl: "https://api.openai.com/v1",
      supportsModelsEndpoint: true,
    },
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://github.com/ii-agent/ii-agent",
    apiKeyUrl: "https://platform.openai.com/api-keys",
    meta: {
      apiFormat: "openai_chat",
      isNewApi: true,
    },
  },
  {
    id: "iiagent_deepseek",
    name: "IIAgent - DeepSeek",
    category: "cn_official",
    transport: {
      formats: ["anthropic", "openai_chat"],
      baseUrl: "https://api.deepseek.com",
      supportsModelsEndpoint: true,
    },
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    meta: {
      apiFormat: "anthropic",
      isNewApi: true,
    },
  },
  {
    id: "iiagent_zhipu",
    name: "IIAgent - Zhipu GLM",
    category: "cn_official",
    transport: {
      formats: ["anthropic", "openai_chat"],
      baseUrl: "https://open.bigmodel.cn/api/paas/v4",
      supportsModelsEndpoint: true,
    },
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    isPartner: true,
    partnerPromotionKey: "zhipu",
    meta: {
      apiFormat: "anthropic",
      isNewApi: true,
    },
  },
  {
    id: "iiagent_zhipu_en",
    name: "IIAgent - Zhipu GLM en",
    category: "cn_official",
    transport: {
      formats: ["anthropic", "openai_chat"],
      baseUrl: "https://api.z.ai/v1",
      supportsModelsEndpoint: true,
    },
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://z.ai",
    apiKeyUrl: "https://z.ai/subscribe?ic=8JVLJQFSKB",
    isPartner: true,
    partnerPromotionKey: "zhipu",
    meta: {
      apiFormat: "anthropic",
      isNewApi: true,
    },
  },
  {
    id: "iiagent_bailian",
    name: "IIAgent - Bailian",
    category: "cn_official",
    transport: {
      formats: ["anthropic", "openai_chat"],
      baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      supportsModelsEndpoint: true,
    },
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://bailian.console.aliyun.com",
    meta: {
      apiFormat: "anthropic",
      isNewApi: true,
    },
  },
  {
    id: "iiagent_kimi",
    name: "IIAgent - Kimi",
    category: "cn_official",
    transport: {
      formats: ["anthropic", "openai_chat"],
      baseUrl: "https://api.moonshot.cn/v1",
      supportsModelsEndpoint: true,
    },
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://platform.moonshot.cn/console",
    apiKeyUrl: "https://platform.moonshot.cn/console/api-keys",
    meta: {
      apiFormat: "anthropic",
      isNewApi: true,
    },
  },
  {
    id: "iiagent_minimax",
    name: "IIAgent - MiniMax",
    category: "cn_official",
    transport: {
      formats: ["anthropic", "openai_chat"],
      baseUrl: "https://api.minimaxi.com/v1",
      supportsModelsEndpoint: true,
    },
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://platform.minimaxi.com",
    apiKeyUrl: "https://platform.minimaxi.com/subscribe/coding-plan",
    isPartner: true,
    partnerPromotionKey: "minimax_cn",
    meta: {
      apiFormat: "anthropic",
      isNewApi: true,
    },
  },
  {
    id: "iiagent_minimax_en",
    name: "IIAgent - MiniMax en",
    category: "cn_official",
    transport: {
      formats: ["anthropic", "openai_chat"],
      baseUrl: "https://api.minimax.io/v1",
      supportsModelsEndpoint: true,
    },
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://api.minimax.io",
    meta: {
      apiFormat: "anthropic",
      isNewApi: true,
    },
  },
  {
    id: "iiagent_custom_anthropic",
    name: "IIAgent - Custom Anthropic",
    category: "custom",
    transport: {
      formats: ["anthropic", "openai_chat"],
      baseUrl: "",
      supportsModelsEndpoint: true,
    },
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://github.com/ii-agent/ii-agent",
    meta: {
      apiFormat: "anthropic",
      isNewApi: true,
    },
  },
  {
    id: "iiagent_custom_openai",
    name: "IIAgent - Custom OpenAI",
    category: "custom",
    transport: {
      formats: ["openai_chat", "openai_responses"],
      baseUrl: "",
      supportsModelsEndpoint: true,
    },
    icon: "iiagent",
    iconColor: "#6366F1",
    websiteUrl: "https://github.com/ii-agent/ii-agent",
    meta: {
      apiFormat: "openai_chat",
      isNewApi: true,
    },
  },
];

export default iiAgentPresets;
