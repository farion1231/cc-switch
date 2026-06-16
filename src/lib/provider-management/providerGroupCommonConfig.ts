import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import {
  extractCodexModelName,
  setCodexBaseUrl,
  setCodexModelName,
} from "@/utils/providerConfigUtils";
import {
  extractProviderSummary,
  maskSecret,
} from "@/lib/provider-management/providerSummary";

export type GroupCommonConfigKey =
  | "apiKey"
  | "baseUrl"
  | "modelMapping"
  | "apiFormat"
  | "customUserAgent";

export interface GroupCommonConfigCandidate {
  key: GroupCommonConfigKey;
  label: string;
  value: unknown;
  displayValue: string;
}

export type GroupCommonConfigCandidates = Partial<
  Record<GroupCommonConfigKey, GroupCommonConfigCandidate>
>;

type PlainRecord = Record<string, unknown>;

const isRecord = (value: unknown): value is PlainRecord =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const stringValue = (value: unknown): string | undefined => {
  if (typeof value !== "string") return undefined;
  const trimmed = value.trim();
  return trimmed ? trimmed : undefined;
};

const cloneRecord = (value: unknown): PlainRecord =>
  isRecord(value) ? JSON.parse(JSON.stringify(value)) : {};

const ensureRecord = (record: PlainRecord, key: string): PlainRecord => {
  if (!isRecord(record[key])) {
    record[key] = {};
  }
  return record[key] as PlainRecord;
};

const candidate = (
  key: GroupCommonConfigKey,
  label: string,
  value: unknown,
  displayValue?: string,
): GroupCommonConfigCandidate | undefined => {
  const fallback =
    typeof value === "string"
      ? value.trim()
      : value === undefined || value === null
        ? ""
        : JSON.stringify(value);
  const display = displayValue ?? fallback;
  if (!display) return undefined;
  return {
    key,
    label,
    value,
    displayValue: display,
  };
};

const claudeApiKey = (settingsConfig: PlainRecord) => {
  const env = isRecord(settingsConfig.env) ? settingsConfig.env : {};
  if (stringValue(env.ANTHROPIC_AUTH_TOKEN)) {
    return {
      field: "ANTHROPIC_AUTH_TOKEN",
      value: stringValue(env.ANTHROPIC_AUTH_TOKEN)!,
    };
  }
  if (stringValue(env.ANTHROPIC_API_KEY)) {
    return {
      field: "ANTHROPIC_API_KEY",
      value: stringValue(env.ANTHROPIC_API_KEY)!,
    };
  }
  return undefined;
};

const getApiKey = (provider: Provider, appId: AppId) => {
  const settingsConfig = isRecord(provider.settingsConfig)
    ? provider.settingsConfig
    : {};
  if (appId === "claude" || appId === "claude-desktop") {
    return claudeApiKey(settingsConfig);
  }
  if (appId === "codex") {
    const auth = isRecord(settingsConfig.auth) ? settingsConfig.auth : {};
    const value = stringValue(auth.OPENAI_API_KEY);
    return value ? { field: "OPENAI_API_KEY", value } : undefined;
  }
  if (appId === "gemini") {
    const env = isRecord(settingsConfig.env) ? settingsConfig.env : {};
    const value = stringValue(env.GEMINI_API_KEY);
    return value ? { field: "GEMINI_API_KEY", value } : undefined;
  }
  const value = stringValue(settingsConfig.apiKey);
  return value ? { field: "apiKey", value } : undefined;
};

const getBaseUrl = (provider: Provider, appId: AppId) =>
  extractProviderSummary(provider, appId).baseUrl;

const getClaudeModelMapping = (settingsConfig: PlainRecord) => {
  const env = isRecord(settingsConfig.env) ? settingsConfig.env : {};
  const mapping: PlainRecord = {};
  [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
  ].forEach((key) => {
    const value = stringValue(env[key]);
    if (value) mapping[key] = value;
  });
  return Object.keys(mapping).length ? mapping : undefined;
};

const getModelMapping = (provider: Provider, appId: AppId) => {
  const settingsConfig = isRecord(provider.settingsConfig)
    ? provider.settingsConfig
    : {};
  if (appId === "claude" || appId === "claude-desktop") {
    return getClaudeModelMapping(settingsConfig);
  }
  if (appId === "codex") {
    return extractCodexModelName(stringValue(settingsConfig.config));
  }
  if (appId === "gemini") {
    const env = isRecord(settingsConfig.env) ? settingsConfig.env : {};
    return stringValue(env.GEMINI_MODEL);
  }
  if (Array.isArray(settingsConfig.models) || isRecord(settingsConfig.models)) {
    return settingsConfig.models;
  }
  return undefined;
};

const modelDisplayValue = (provider: Provider, appId: AppId) =>
  extractProviderSummary(provider, appId).modelSummary;

export const getGroupCommonConfigCandidates = (
  provider: Provider,
  appId: AppId,
): GroupCommonConfigCandidates => {
  const apiKey = getApiKey(provider, appId);
  const baseUrl = getBaseUrl(provider, appId);
  const modelMapping = getModelMapping(provider, appId);
  const apiFormat = stringValue(provider.meta?.apiFormat);
  const customUserAgent = stringValue(provider.meta?.customUserAgent);

  return {
    apiKey: apiKey
      ? candidate("apiKey", "API key", apiKey.value, maskSecret(apiKey.value))
      : undefined,
    baseUrl: candidate("baseUrl", "Base URL", baseUrl),
    modelMapping: candidate(
      "modelMapping",
      "Model mapping",
      modelMapping,
      modelDisplayValue(provider, appId),
    ),
    apiFormat: candidate("apiFormat", "API format", apiFormat),
    customUserAgent: candidate(
      "customUserAgent",
      "User-Agent",
      customUserAgent,
    ),
  };
};

const applyApiKey = (
  targetConfig: PlainRecord,
  source: Provider,
  appId: AppId,
) => {
  const apiKey = getApiKey(source, appId);
  if (!apiKey) return;
  if (appId === "claude" || appId === "claude-desktop") {
    const env = ensureRecord(targetConfig, "env");
    const field =
      "ANTHROPIC_AUTH_TOKEN" in env
        ? "ANTHROPIC_AUTH_TOKEN"
        : "ANTHROPIC_API_KEY" in env
          ? "ANTHROPIC_API_KEY"
          : apiKey.field;
    env[field] = apiKey.value;
    if (field === "ANTHROPIC_AUTH_TOKEN") {
      delete env.ANTHROPIC_API_KEY;
    } else {
      delete env.ANTHROPIC_AUTH_TOKEN;
    }
    return;
  }
  if (appId === "codex") {
    const auth = ensureRecord(targetConfig, "auth");
    auth.OPENAI_API_KEY = apiKey.value;
    return;
  }
  if (appId === "gemini") {
    const env = ensureRecord(targetConfig, "env");
    env.GEMINI_API_KEY = apiKey.value;
    return;
  }
  targetConfig.apiKey = apiKey.value;
};

const applyBaseUrl = (
  targetConfig: PlainRecord,
  source: Provider,
  appId: AppId,
) => {
  const baseUrl = getBaseUrl(source, appId);
  if (!baseUrl) return;
  if (appId === "claude" || appId === "claude-desktop") {
    ensureRecord(targetConfig, "env").ANTHROPIC_BASE_URL = baseUrl;
    return;
  }
  if (appId === "codex") {
    targetConfig.config = setCodexBaseUrl(
      stringValue(targetConfig.config) ?? "",
      baseUrl,
    );
    return;
  }
  if (appId === "gemini") {
    ensureRecord(targetConfig, "env").GOOGLE_GEMINI_BASE_URL = baseUrl;
    return;
  }
  targetConfig.baseUrl = baseUrl;
};

const applyModelMapping = (
  targetConfig: PlainRecord,
  source: Provider,
  appId: AppId,
) => {
  const mapping = getModelMapping(source, appId);
  if (!mapping) return;

  if (appId === "claude" || appId === "claude-desktop") {
    const env = ensureRecord(targetConfig, "env");
    Object.entries(mapping as PlainRecord).forEach(([key, value]) => {
      env[key] = value;
    });
    return;
  }
  if (appId === "codex" && typeof mapping === "string") {
    targetConfig.config = setCodexModelName(
      stringValue(targetConfig.config) ?? "",
      mapping,
    );
    return;
  }
  if (appId === "gemini" && typeof mapping === "string") {
    ensureRecord(targetConfig, "env").GEMINI_MODEL = mapping;
    return;
  }
  targetConfig.models = JSON.parse(JSON.stringify(mapping));
};

export const applyGroupCommonConfig = (
  target: Provider,
  source: Provider,
  appId: AppId,
  keys: GroupCommonConfigKey[],
): Provider => {
  const settingsConfig = cloneRecord(target.settingsConfig);
  const enabled = Object.fromEntries(keys.map((key) => [key, true]));

  keys.forEach((key) => {
    if (key === "apiKey") {
      applyApiKey(settingsConfig, source, appId);
    } else if (key === "baseUrl") {
      applyBaseUrl(settingsConfig, source, appId);
    } else if (key === "modelMapping") {
      applyModelMapping(settingsConfig, source, appId);
    }
  });

  const meta = {
    ...(target.meta ?? {}),
    groupCommonConfigEnabled: enabled,
  };

  if (keys.includes("apiFormat") && source.meta?.apiFormat) {
    meta.apiFormat = source.meta.apiFormat;
  }
  if (keys.includes("customUserAgent") && source.meta?.customUserAgent) {
    meta.customUserAgent = source.meta.customUserAgent;
  }

  return {
    ...target,
    settingsConfig,
    meta,
  };
};
