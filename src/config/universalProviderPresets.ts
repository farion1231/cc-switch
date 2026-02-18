import type {
  UniversalProvider,
  UniversalProviderApps,
  UniversalProviderModels,
} from "@/types";
import {
  ENDPOINTS,
  deriveApps,
  isUniversal,
  type ProviderEndpoint,
} from "./capabilities";

export interface UniversalProviderPreset {
  name: string;
  providerType: string;
  defaultApps: UniversalProviderApps;
  defaultModels: UniversalProviderModels;
  websiteUrl?: string;
  icon?: string;
  iconColor?: string;
  description?: string;
  isCustomTemplate?: boolean;
  meta?: import("@/types").ProviderMeta;
  endpointId?: string;
}

const NEWAPI_DEFAULT_MODELS: UniversalProviderModels = {
  claude: {
    model: "claude-3-5-sonnet-20240620",
    haikuModel: "claude-3-haiku-20240307",
    sonnetModel: "claude-3-5-sonnet-20240620",
    opusModel: "claude-3-opus-20240229",
  },
  codex: {
    model: "gpt-4o",
    reasoningEffort: "high",
  },
  gemini: {
    model: "gemini-1.5-pro",
  },
};

export const universalProviderPresets: UniversalProviderPreset[] = [
  {
    name: "NewAPI",
    providerType: "newapi",
    defaultApps: { claude: true, codex: true, gemini: true },
    defaultModels: NEWAPI_DEFAULT_MODELS,
    websiteUrl: "https://www.newapi.ai",
    icon: "newapi",
    iconColor: "#00A67E",
    description:
      "NewAPI 是一个可自部署的 API 网关，支持 Anthropic、OpenAI、Gemini 等多种协议",
    meta: { isNewApi: true },
  },
  {
    name: "自定义网关",
    providerType: "custom_gateway",
    defaultApps: { claude: true, codex: true, gemini: true },
    defaultModels: NEWAPI_DEFAULT_MODELS,
    icon: "openai",
    iconColor: "#6366F1",
    description: "自定义配置的 API 网关",
    isCustomTemplate: true,
  },
];

export const UNIVERSAL_ENDPOINTS = ENDPOINTS.filter(isUniversal);

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
    models: JSON.parse(JSON.stringify(preset.defaultModels)),
    websiteUrl: preset.websiteUrl,
    icon: preset.icon,
    iconColor: preset.iconColor,
    meta: preset.meta,
    createdAt: Date.now(),
  };
}

export function createUniversalProviderFromEndpoint(
  endpoint: ProviderEndpoint,
  apiKey: string,
  customName?: string,
): UniversalProvider | null {
  if (!isUniversal(endpoint)) return null;

  const derived = deriveApps(endpoint);
  const apps: UniversalProviderApps = {
    claude: derived.claude,
    codex: derived.codex,
    gemini: derived.gemini,
  };
  return {
    id: endpoint.id,
    name: customName || endpoint.name,
    providerType: endpoint.id,
    apps,
    baseUrl: endpoint.transport.baseUrl,
    apiKey,
    models: JSON.parse(JSON.stringify(NEWAPI_DEFAULT_MODELS)),
    websiteUrl: endpoint.websiteUrl,
    icon: endpoint.icon,
    iconColor: endpoint.iconColor,
    createdAt: Date.now(),
  };
}

export function getPresetDisplayName(preset: UniversalProviderPreset): string {
  return preset.name;
}

export function findPresetByType(
  providerType: string,
): UniversalProviderPreset | undefined {
  return universalProviderPresets.find((p) => p.providerType === providerType);
}
