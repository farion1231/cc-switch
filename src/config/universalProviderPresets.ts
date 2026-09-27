/**
 * 统一供应商（Universal Provider）预设配置
 *
 * 统一供应商是跨应用共享的配置，修改后会自动同步到 Claude、Codex、Gemini 三个应用。
 * 适用于 NewAPI 等支持多种协议的 API 网关。
 */

import type {
  ProviderMeta,
  UniversalProvider,
  UniversalProviderApps,
  UniversalProviderModels,
} from "@/types";
import { deepClone } from "@/utils/deepClone";

/**
 * 统一供应商预设接口
 */
export interface UniversalProviderPreset {
  /** 预设名称 */
  name: string;
  /** 供应商类型标识 */
  providerType: string;
  /** 默认启用的应用 */
  defaultApps: UniversalProviderApps;
  /** 默认模型配置 */
  defaultModels: UniversalProviderModels;
  /** 默认 API 地址 */
  defaultBaseUrl?: string;
  /** 网站链接 */
  websiteUrl?: string;
  /** 图标名称 */
  icon?: string;
  /** 图标颜色 */
  iconColor?: string;
  /** 描述 */
  description?: string;
  /** 同步到各客户端 Provider 的元数据 */
  meta?: ProviderMeta;
  /** 是否为自定义模板（允许用户完全自定义） */
  isCustomTemplate?: boolean;
}

/**
 * NewAPI 默认模型配置
 */
const NEWAPI_DEFAULT_MODELS: UniversalProviderModels = {
  claude: {
    model: "claude-sonnet-5",
    haikuModel: "claude-haiku-4-5-20251001",
    sonnetModel: "claude-sonnet-5",
    opusModel: "claude-opus-5",
  },
  codex: {
    model: "gpt-5.6-sol",
    reasoningEffort: "high",
  },
  gemini: {
    model: "gemini-3.6-flash",
  },
  hermes: {
    model: "gpt-5.6-sol",
  },
};

/**
 * 统一供应商预设列表
 */
export const universalProviderPresets: UniversalProviderPreset[] = [
  {
    name: "CodeBuddy",
    providerType: "codebuddy",
    defaultApps: {
      claude: true,
      codex: true,
      gemini: false,
      hermes: true,
    },
    defaultModels: {
      claude: {
        model: "deepseek-v4-flash",
        haikuModel: "deepseek-v4-flash",
        sonnetModel: "deepseek-v4-flash",
        opusModel: "deepseek-v4-flash",
      },
      codex: {
        model: "deepseek-v4-flash",
        reasoningEffort: "high",
      },
      hermes: {
        model: "deepseek-v4-flash",
      },
    },
    defaultBaseUrl: "https://copilot.tencent.com/v2/chat/completions",
    websiteUrl: "https://www.codebuddy.cn",
    icon: "codebuddy",
    iconColor: "#7C3AED",
    description:
      "腾讯 CodeBuddy 中国版 API，通过 CC Switch 本地路由供 Claude Code 和 Codex 使用",
    meta: {
      apiFormat: "openai_chat",
      isFullUrl: true,
      promptCacheRouting: "disabled",
      codexChatReasoning: {
        supportsThinking: false,
        supportsEffort: false,
        thinkingParam: "none",
        effortParam: "none",
      },
    },
  },
  {
    name: "NewAPI",
    providerType: "newapi",
    defaultApps: {
      claude: true,
      codex: true,
      gemini: true,
      hermes: true,
    },
    defaultModels: NEWAPI_DEFAULT_MODELS,
    websiteUrl: "https://www.newapi.pro",
    icon: "newapi",
    iconColor: "#00A67E",
    description:
      "NewAPI 是一个可自部署的 API 网关，支持 Anthropic、OpenAI、Gemini 等多种协议",
  },
  {
    name: "自定义网关",
    providerType: "custom_gateway",
    defaultApps: {
      claude: true,
      codex: true,
      gemini: true,
      hermes: true,
    },
    defaultModels: NEWAPI_DEFAULT_MODELS,
    icon: "openai",
    iconColor: "#6366F1",
    description: "自定义配置的 API 网关",
    isCustomTemplate: true,
  },
];

export function normalizeUniversalCodexBaseUrl(baseUrl: string): string {
  const baseTrimmed = baseUrl.replace(/\/+$/, "");
  const authority = baseTrimmed.split("://", 2)[1] ?? baseTrimmed;
  return baseTrimmed.endsWith("/v1") || authority.includes("/")
    ? baseTrimmed
    : `${baseTrimmed}/v1`;
}

export function normalizeUniversalHermesBaseUrl(baseUrl: string): string {
  const trimmed = baseUrl.replace(/\/+$/, "");
  return trimmed.endsWith("/chat/completions")
    ? trimmed.slice(0, -"/chat/completions".length)
    : trimmed;
}

/**
 * 根据预设创建统一供应商
 */
export function createUniversalProviderFromPreset(
  preset: UniversalProviderPreset,
  id: string,
  baseUrl: string,
  apiKey: string,
  customName?: string,
): UniversalProvider {
  return {
    id,
    name: customName || preset.name,
    providerType: preset.providerType,
    apps: { ...preset.defaultApps },
    baseUrl,
    apiKey,
    models: deepClone(preset.defaultModels),
    websiteUrl: preset.websiteUrl,
    icon: preset.icon,
    iconColor: preset.iconColor,
    meta: preset.meta ? deepClone(preset.meta) : undefined,
    createdAt: Date.now(),
  };
}

/**
 * 获取预设的显示名称（用于 UI）
 */
export function getPresetDisplayName(preset: UniversalProviderPreset): string {
  return preset.name;
}

/**
 * 根据类型查找预设
 */
export function findPresetByType(
  providerType: string,
): UniversalProviderPreset | undefined {
  return universalProviderPresets.find((p) => p.providerType === providerType);
}
