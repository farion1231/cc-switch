import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";
import { extractCodexModelName } from "@/utils/providerConfigUtils";

const trimToDefined = (value: unknown): string | undefined => {
  if (typeof value !== "string") return undefined;
  const trimmed = value.trim();
  return trimmed ? trimmed : undefined;
};

const getEnvValue = (provider: Provider, key: string): string | undefined => {
  const env = provider.settingsConfig?.env;
  if (!env || typeof env !== "object") return undefined;
  return trimToDefined((env as Record<string, unknown>)[key]);
};

const getCodexModel = (provider: Provider): string | undefined => {
  const config = provider.settingsConfig?.config;
  if (typeof config === "string") {
    return extractCodexModelName(config);
  }
  if (config && typeof config === "object") {
    return trimToDefined((config as Record<string, unknown>).model);
  }
  return undefined;
};

const getOpenCodeModel = (provider: Provider): string | undefined => {
  const config = provider.settingsConfig;
  if (!config || typeof config !== "object") return undefined;

  const directModel = trimToDefined((config as Record<string, unknown>).model);
  if (directModel) return directModel;

  const models = (config as Record<string, unknown>).models;
  if (models && typeof models === "object" && !Array.isArray(models)) {
    const firstModelKey = Object.keys(models as Record<string, unknown>)[0];
    if (firstModelKey) return firstModelKey;
  }

  const agents = (config as Record<string, unknown>).agents;
  if (agents && typeof agents === "object" && !Array.isArray(agents)) {
    const firstAgentModel = Object.values(
      agents as Record<string, unknown>,
    ).find((agent) => {
      if (!agent || typeof agent !== "object") return false;
      return !!trimToDefined((agent as Record<string, unknown>).model);
    }) as Record<string, unknown> | undefined;

    if (firstAgentModel) {
      return trimToDefined(firstAgentModel.model);
    }
  }

  return undefined;
};

const getOpenClawModel = (provider: Provider): string | undefined => {
  const models = provider.settingsConfig?.models;
  if (!Array.isArray(models) || models.length === 0) return undefined;

  const first = models[0];
  if (!first || typeof first !== "object") return undefined;
  return trimToDefined((first as Record<string, unknown>).id);
};

export const extractProviderCurrentModel = (
  provider: Provider,
  appId: AppId,
): string | undefined => {
  switch (appId) {
    case "claude":
      return (
        getEnvValue(provider, "ANTHROPIC_MODEL") ||
        getEnvValue(provider, "ANTHROPIC_DEFAULT_SONNET_MODEL") ||
        getEnvValue(provider, "ANTHROPIC_DEFAULT_HAIKU_MODEL") ||
        getEnvValue(provider, "ANTHROPIC_DEFAULT_OPUS_MODEL")
      );
    case "codex":
      return getCodexModel(provider);
    case "gemini":
      return getEnvValue(provider, "GEMINI_MODEL");
    case "opencode":
      return getOpenCodeModel(provider);
    case "openclaw":
      return getOpenClawModel(provider);
    default:
      return undefined;
  }
};
