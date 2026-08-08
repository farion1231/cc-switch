import { parse as parseToml, stringify as stringifyToml } from "smol-toml";

export const GROK_BUILD_DEFAULT_MODEL = "grok-4.5";
export const GROK_BUILD_DEFAULT_API_BACKEND = "responses";
export const GROK_BUILD_DEFAULT_CONTEXT_WINDOW = 500000;

export interface GrokBuildConfigValues {
  /** Client-visible profile selected by [models].default. */
  model: string;
  /** Real model sent to the upstream provider. */
  upstreamModel?: string;
  baseUrl: string;
  name: string;
  apiKey: string;
  envKey?: string;
  apiBackend: string;
  contextWindow: number;
}

export interface GrokBuildConfigOptions {
  baseUrlMode?: "model" | "models_endpoint";
  webSearchModel?: string;
  supportsBackendSearch?: boolean;
}

const asRecord = (value: unknown): Record<string, unknown> | undefined =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;

const asString = (value: unknown, fallback = "") =>
  typeof value === "string" ? value : fallback;

export function parseGrokBuildConfig(
  configToml: string | undefined,
  fallbackName = "",
): GrokBuildConfigValues {
  const fallback: GrokBuildConfigValues = {
    model: GROK_BUILD_DEFAULT_MODEL,
    upstreamModel: GROK_BUILD_DEFAULT_MODEL,
    baseUrl: "",
    name: fallbackName,
    apiKey: "",
    apiBackend: GROK_BUILD_DEFAULT_API_BACKEND,
    contextWindow: GROK_BUILD_DEFAULT_CONTEXT_WINDOW,
  };

  if (!configToml?.trim()) return fallback;

  try {
    const root = asRecord(parseToml(configToml));
    const models = asRecord(root?.models);
    const defaultModel = asString(models?.default, GROK_BUILD_DEFAULT_MODEL);
    const modelTables = asRecord(root?.model);
    const selectedModel = asRecord(modelTables?.[defaultModel]);
    const endpoints = asRecord(root?.endpoints);
    const rawContextWindow = selectedModel?.context_window;

    return {
      model: defaultModel,
      upstreamModel: asString(selectedModel?.model, defaultModel),
      baseUrl:
        asString(selectedModel?.base_url).trim() ||
        asString(endpoints?.models_base_url),
      name: asString(selectedModel?.name, fallbackName),
      apiKey: asString(selectedModel?.api_key),
      envKey: asString(selectedModel?.env_key),
      apiBackend: asString(
        selectedModel?.api_backend,
        GROK_BUILD_DEFAULT_API_BACKEND,
      ),
      contextWindow:
        typeof rawContextWindow === "number" &&
        Number.isInteger(rawContextWindow) &&
        rawContextWindow > 0
          ? rawContextWindow
          : GROK_BUILD_DEFAULT_CONTEXT_WINDOW,
    };
  } catch {
    return fallback;
  }
}

export function buildGrokBuildConfig(
  values: GrokBuildConfigValues,
  options?: GrokBuildConfigOptions,
): string {
  return updateGrokBuildConfig(undefined, values, options);
}

export function updateGrokBuildConfig(
  configToml: string | undefined,
  values: GrokBuildConfigValues,
  options: GrokBuildConfigOptions = {},
): string {
  const profile = values.model.trim() || GROK_BUILD_DEFAULT_MODEL;
  const upstreamModel = values.upstreamModel?.trim() || profile;
  let config: Record<string, unknown> = {};

  try {
    config = asRecord(configToml?.trim() ? parseToml(configToml) : {}) ?? {};
  } catch {
    config = {};
  }

  const existingModels = asRecord(config.models) ?? {};
  const previousProfile = asString(existingModels.default, profile);
  const updatedModels: Record<string, unknown> = {
    ...existingModels,
    default: profile,
  };
  if (options.webSearchModel !== undefined) {
    const webSearchModel = options.webSearchModel.trim();
    if (webSearchModel) updatedModels.web_search = webSearchModel;
    else delete updatedModels.web_search;
  }
  config.models = updatedModels;

  const modelTables = asRecord(config.model) ?? {};
  const existingSelected =
    asRecord(modelTables[profile]) ??
    asRecord(modelTables[previousProfile]) ??
    {};
  const existingEndpoints = asRecord(config.endpoints) ?? {};
  const baseUrlMode =
    options.baseUrlMode ??
    (!asString(existingSelected.base_url).trim() &&
    asString(existingEndpoints.models_base_url).trim()
      ? "models_endpoint"
      : "model");
  const apiKey = values.apiKey.trim();
  const envKey =
    values.envKey?.trim() || asString(existingSelected.env_key).trim();
  const updatedSelected: Record<string, unknown> = {
    ...existingSelected,
    model: upstreamModel,
    name: values.name.trim(),
    api_backend: values.apiBackend.trim() || GROK_BUILD_DEFAULT_API_BACKEND,
    context_window:
      Number.isInteger(values.contextWindow) && values.contextWindow > 0
        ? values.contextWindow
        : GROK_BUILD_DEFAULT_CONTEXT_WINDOW,
  };
  if (baseUrlMode === "models_endpoint") {
    delete updatedSelected.base_url;
    config.endpoints = {
      ...existingEndpoints,
      models_base_url: values.baseUrl.trim(),
    };
  } else {
    updatedSelected.base_url = values.baseUrl.trim();
  }
  if (options.supportsBackendSearch !== undefined) {
    updatedSelected.supports_backend_search = options.supportsBackendSearch;
  }
  if (apiKey) updatedSelected.api_key = apiKey;
  else delete updatedSelected.api_key;
  if (envKey) updatedSelected.env_key = envKey;
  else delete updatedSelected.env_key;

  config.model = {
    ...modelTables,
    [profile]: updatedSelected,
  };

  if (previousProfile !== profile && previousProfile in modelTables) {
    delete (config.model as Record<string, unknown>)[previousProfile];
  }

  return `${stringifyToml(config).trim()}\n`;
}

export function validateGrokBuildConfig(configToml: string): string | null {
  if (!configToml.trim()) return "config.toml must not be empty";
  try {
    const root = asRecord(parseToml(configToml));
    const models = asRecord(root?.models);
    const profile = asString(models?.default).trim();
    const selected = asRecord(asRecord(root?.model)?.[profile]);
    if (!profile || !selected) return "Missing [models] default model table";
    for (const field of ["model", "name", "api_backend"]) {
      if (!asString(selected[field]).trim()) return `Missing ${field}`;
    }
    const modelBaseUrl = asString(selected.base_url).trim();
    const modelsBaseUrl = asString(
      asRecord(root?.endpoints)?.models_base_url,
    ).trim();
    if (!modelBaseUrl && !modelsBaseUrl) {
      return "Missing base_url or endpoints.models_base_url";
    }
    if (
      !asString(selected.api_key).trim() &&
      !asString(selected.env_key).trim()
    ) {
      return "Missing api_key or env_key";
    }
    const contextWindow = selected.context_window;
    if (
      typeof contextWindow !== "number" ||
      !Number.isInteger(contextWindow) ||
      contextWindow <= 0
    ) {
      return "context_window must be a positive integer";
    }
    return null;
  } catch (error) {
    return error instanceof Error ? error.message : "Invalid TOML";
  }
}

export function extractGrokBuildBaseUrl(configToml: string): string {
  return parseGrokBuildConfig(configToml).baseUrl;
}
