/**
 * 统一供应商（Universal Provider）预设配置
 *
 * 统一供应商是跨应用共享的配置，修改后会自动同步到 Claude、Codex、Gemini 三个应用。
 * 适用于 NewAPI 等支持多种协议的 API 网关。
 */

import type {
  UniversalProvider,
  UniversalProviderApps,
  UniversalProviderModels,
} from "@/types";

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
  /** 网站链接 */
  websiteUrl?: string;
  /** 图标名称 */
  icon?: string;
  /** 图标颜色 */
  iconColor?: string;
  /** 描述 */
  description?: string;
  /** 是否为自定义模板（允许用户完全自定义） */
  isCustomTemplate?: boolean;
}

/**
 * NewAPI 默认模型配置
 */
const NEWAPI_DEFAULT_MODELS: UniversalProviderModels = {
  claude: {
    model: "claude-sonnet-4-20250514",
    haikuModel: "claude-haiku-4-20250514",
    sonnetModel: "claude-sonnet-4-20250514",
    opusModel: "claude-sonnet-4-20250514",
  },
  codex: {
    model: "gpt-4o",
    reasoningEffort: "high",
  },
  gemini: {
    model: "gemini-2.5-pro",
  },
};

/**
 * 统一供应商预设列表
 */
export const universalProviderPresets: UniversalProviderPreset[] = [
  {
    name: "NewAPI",
    providerType: "newapi",
    defaultApps: {
      claude: true,
      codex: true,
      gemini: true,
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
    },
    defaultModels: NEWAPI_DEFAULT_MODELS,
    icon: "openai",
    iconColor: "#6366F1",
    description: "自定义配置的 API 网关",
    isCustomTemplate: true,
  },
];

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
    models: JSON.parse(JSON.stringify(preset.defaultModels)), // Deep copy
    websiteUrl: preset.websiteUrl,
    icon: preset.icon,
    iconColor: preset.iconColor,
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
