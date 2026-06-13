import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import {
  extractCodexBaseUrl,
  extractCodexModelName,
} from "@/utils/providerConfigUtils";

export interface ProviderSummary {
  baseUrl?: string;
  baseUrlHost?: string;
  apiKeyFingerprint?: string;
  modelSummary?: string;
  modelValues: string[];
  apiFormat?: string;
  providerType?: string;
  searchText: string[];
}

type PlainRecord = Record<string, unknown>;

const SECRET_KEY_PATTERN =
  /(api[_-]?key|auth[_-]?token|access[_-]?token|bearer|secret|password|token)$/i;

const MODEL_KEY_PATTERN = /(^|[_-])(model|models|sonnet|opus|haiku)([_-]|$)/i;

const isRecord = (value: unknown): value is PlainRecord =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const stringValue = (value: unknown): string | undefined => {
  if (typeof value !== "string") return undefined;
  const trimmed = value.trim();
  return trimmed ? trimmed : undefined;
};

export const maskSecret = (value: unknown): string | undefined => {
  const secret = stringValue(value);
  if (!secret) return undefined;

  if (secret.length <= 3) {
    return "*".repeat(secret.length);
  }

  if (secret.length <= 8) {
    return `${secret.slice(0, 3)}...${secret.slice(-3)}`;
  }

  return `${secret.slice(0, 6)}...${secret.slice(-4)}`;
};

const safeHost = (url: string | undefined): string | undefined => {
  if (!url) return undefined;
  try {
    return new URL(url).host;
  } catch {
    return undefined;
  }
};

const pushUnique = (target: string[], value: unknown) => {
  const text = stringValue(value);
  if (!text) return;
  if (!target.includes(text)) {
    target.push(text);
  }
};

const readPath = (
  value: unknown,
  path: Array<string>,
): string | undefined => {
  let cursor = value;
  for (const segment of path) {
    if (!isRecord(cursor)) return undefined;
    cursor = cursor[segment];
  }
  return stringValue(cursor);
};

const firstPath = (
  value: unknown,
  paths: Array<Array<string>>,
): string | undefined => {
  for (const path of paths) {
    const result = readPath(value, path);
    if (result) return result;
  }
  return undefined;
};

const collectSecretFingerprints = (
  value: unknown,
  output: string[],
  keyName = "",
) => {
  if (Array.isArray(value)) {
    value.forEach((item) => collectSecretFingerprints(item, output, keyName));
    return;
  }

  if (isRecord(value)) {
    for (const [key, child] of Object.entries(value)) {
      collectSecretFingerprints(child, output, key);
    }
    return;
  }

  if (!SECRET_KEY_PATTERN.test(keyName)) return;
  pushUnique(output, maskSecret(value));
};

const collectModelValues = (
  value: unknown,
  output: string[],
  keyName = "",
) => {
  if (Array.isArray(value)) {
    value.forEach((item) => collectModelValues(item, output, keyName));
    return;
  }

  if (isRecord(value)) {
    for (const [key, child] of Object.entries(value)) {
      collectModelValues(child, output, key);
    }
    return;
  }

  if (!MODEL_KEY_PATTERN.test(keyName)) return;
  pushUnique(output, value);
};

const claudeModelPairs = (settingsConfig: PlainRecord) => {
  const env = isRecord(settingsConfig.env) ? settingsConfig.env : {};
  return [
    ["Model", stringValue(env.ANTHROPIC_MODEL)],
    ["Sonnet", stringValue(env.ANTHROPIC_DEFAULT_SONNET_MODEL)],
    ["Opus", stringValue(env.ANTHROPIC_DEFAULT_OPUS_MODEL)],
    ["Haiku", stringValue(env.ANTHROPIC_DEFAULT_HAIKU_MODEL)],
  ].filter((entry): entry is [string, string] => Boolean(entry[1]));
};

const geminiModelPairs = (settingsConfig: PlainRecord) => {
  const env = isRecord(settingsConfig.env) ? settingsConfig.env : {};
  const model = stringValue(env.GEMINI_MODEL);
  return model ? ([["Model", model]] as Array<[string, string]>) : [];
};

const opencodeModelPairs = (settingsConfig: PlainRecord) => {
  const models = isRecord(settingsConfig.models) ? settingsConfig.models : {};
  return Object.keys(models)
    .slice(0, 3)
    .map((model) => ["Model", model] as [string, string]);
};

const openclawModelPairs = (settingsConfig: PlainRecord) => {
  const models = Array.isArray(settingsConfig.models)
    ? settingsConfig.models
    : [];
  return models
    .map((item) => {
      if (typeof item === "string") return item;
      if (isRecord(item)) {
        return stringValue(item.id) ?? stringValue(item.name);
      }
      return undefined;
    })
    .filter((model): model is string => Boolean(model))
    .slice(0, 3)
    .map((model) => ["Model", model] as [string, string]);
};

const getBaseUrl = (
  settingsConfig: PlainRecord,
  appId: AppId,
): string | undefined => {
  if (appId === "codex") {
    return extractCodexBaseUrl(stringValue(settingsConfig.config));
  }

  return firstPath(settingsConfig, [
    ["env", "ANTHROPIC_BASE_URL"],
    ["env", "GOOGLE_GEMINI_BASE_URL"],
    ["baseUrl"],
    ["baseURL"],
    ["base_url"],
    ["apiBaseUrl"],
    ["apiBaseURL"],
    ["api", "baseUrl"],
    ["options", "baseURL"],
    ["options", "baseUrl"],
  ]);
};

const getModelPairs = (
  settingsConfig: PlainRecord,
  appId: AppId,
): Array<[string, string]> => {
  if (appId === "claude" || appId === "claude-desktop") {
    return claudeModelPairs(settingsConfig);
  }
  if (appId === "codex") {
    const model = extractCodexModelName(stringValue(settingsConfig.config));
    return model ? [["Model", model]] : [];
  }
  if (appId === "gemini") {
    return geminiModelPairs(settingsConfig);
  }
  if (appId === "openclaw") {
    return openclawModelPairs(settingsConfig);
  }
  if (appId === "opencode") {
    return opencodeModelPairs(settingsConfig);
  }
  return [];
};

export const extractProviderSummary = (
  provider: Provider,
  appId: AppId,
): ProviderSummary => {
  const settingsConfig = isRecord(provider.settingsConfig)
    ? provider.settingsConfig
    : {};
  const meta = isRecord(provider.meta) ? provider.meta : {};
  const baseUrl = getBaseUrl(settingsConfig, appId);
  const baseUrlHost = safeHost(baseUrl);
  const apiKeyFingerprints: string[] = [];
  collectSecretFingerprints(settingsConfig, apiKeyFingerprints);

  const modelPairs = getModelPairs(settingsConfig, appId);
  const modelValues = modelPairs.map(([, value]) => value);
  collectModelValues(settingsConfig, modelValues);

  const apiFormat =
    stringValue(meta.apiFormat) ??
    firstPath(settingsConfig, [["apiFormat"], ["api_format"]]);
  const providerType = stringValue(meta.providerType);
  const modelSummary = modelPairs.length
    ? modelPairs.map(([label, value]) => `${label}=${value}`).join(" ")
    : undefined;

  const searchText: string[] = [];
  [
    provider.id,
    provider.name,
    provider.notes,
    provider.websiteUrl,
    provider.category,
    baseUrl,
    baseUrlHost,
    apiFormat,
    providerType,
    modelSummary,
  ].forEach((value) => pushUnique(searchText, value));
  apiKeyFingerprints.forEach((value) => pushUnique(searchText, value));
  modelValues.forEach((value) => pushUnique(searchText, value));

  return {
    baseUrl,
    baseUrlHost,
    apiKeyFingerprint: apiKeyFingerprints[0],
    modelSummary,
    modelValues,
    apiFormat,
    providerType,
    searchText,
  };
};
