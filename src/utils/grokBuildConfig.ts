import { parse as parseToml, stringify as stringifyToml } from "smol-toml";

export const GROK_BUILD_DEFAULT_MODEL = "grok-4.5";
export const GROK_BUILD_DEFAULT_API_BACKEND = "responses";
export const GROK_BUILD_DEFAULT_CONTEXT_WINDOW = 500000;

export interface GrokBuildExtraModel {
  /** `[model."<profile>"]` key. Defaults to the upstream model id. */
  profile?: string;
  /** Real model id sent to the upstream provider. */
  model: string;
  /** Label shown in Grok's `/model` menu. */
  name?: string;
  contextWindow?: number;
  reasoningLevels?: string[];
  defaultReasoningLevel?: string;
}

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
  /**
   * Other `[model.*]` profiles on the same gateway.
   * Undefined keeps unmanaged profiles untouched; an array replaces them.
   */
  extraModels?: GrokBuildExtraModel[];
  /** Reasoning menu for the default profile. Undefined preserves the existing menu. */
  reasoningLevels?: string[];
  defaultReasoningLevel?: string;
}

const asRecord = (value: unknown): Record<string, unknown> | undefined =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;

const asString = (value: unknown, fallback = "") =>
  typeof value === "string" ? value : fallback;

const GROK_REASONING_LEVELS = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
] as const;

const GROK_REASONING_LABELS: Record<string, string> = {
  none: "None",
  minimal: "Minimal Effort",
  low: "Low Effort",
  medium: "Medium Effort",
  high: "High Effort",
  xhigh: "Extra High Effort",
  max: "Max Effort",
  ultra: "Ultra Effort",
};

const readContextWindow = (value: unknown, fallback: number) =>
  typeof value === "number" && Number.isInteger(value) && value > 0
    ? value
    : fallback;

const readReasoning = (
  table: Record<string, unknown> | undefined,
): { reasoningLevels?: string[]; defaultReasoningLevel?: string } => {
  const raw = table?.reasoning_efforts;
  if (!Array.isArray(raw)) return {};
  const declared = raw.flatMap((entry) => {
    const record = asRecord(entry);
    const value = asString(record?.value).trim();
    return value ? [value] : [];
  });
  const reasoningLevels = GROK_REASONING_LEVELS.filter((level) =>
    declared.includes(level),
  );
  const extras = declared.filter(
    (level) => !(GROK_REASONING_LEVELS as readonly string[]).includes(level),
  );
  const levels = [...reasoningLevels, ...extras];
  if (levels.length === 0) return {};
  const defaultReasoningLevel = raw
    .map((entry) => asRecord(entry))
    .find((entry) => entry?.default === true)?.value;
  const normalizedDefault = asString(defaultReasoningLevel).trim();
  return {
    reasoningLevels: levels,
    ...(normalizedDefault && levels.includes(normalizedDefault)
      ? { defaultReasoningLevel: normalizedDefault }
      : {}),
  };
};

const reasoningEfforts = (
  levels: string[] | undefined,
  defaultLevel: string | undefined,
) => {
  if (!levels || levels.length === 0) return undefined;
  return levels.map((value) => ({
    value,
    label: GROK_REASONING_LABELS[value] ?? value,
    ...(defaultLevel === value ? { default: true } : {}),
  }));
};

const readExtraModels = (
  modelTables: Record<string, unknown> | undefined,
  defaultProfile: string,
  fallbackWindow: number,
): GrokBuildExtraModel[] => {
  if (!modelTables) return [];
  return Object.entries(modelTables).flatMap(([profile, value]) => {
    if (profile === defaultProfile) return [];
    const table = asRecord(value);
    if (!table) return [];
    const model = asString(table.model, profile).trim() || profile;
    const reasoning = readReasoning(table);
    return [
      {
        profile,
        model,
        name: asString(table.name).trim() || model,
        contextWindow: readContextWindow(table.context_window, fallbackWindow),
        ...reasoning,
      },
    ];
  });
};

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
    const contextWindow = readContextWindow(
      selectedModel?.context_window,
      GROK_BUILD_DEFAULT_CONTEXT_WINDOW,
    );

    return {
      model: defaultModel,
      upstreamModel: asString(selectedModel?.model, defaultModel),
      baseUrl: asString(selectedModel?.base_url),
      name: asString(selectedModel?.name, fallbackName),
      apiKey: asString(selectedModel?.api_key),
      envKey: asString(selectedModel?.env_key),
      apiBackend: asString(
        selectedModel?.api_backend,
        GROK_BUILD_DEFAULT_API_BACKEND,
      ),
      contextWindow,
      extraModels: readExtraModels(modelTables, defaultModel, contextWindow),
      ...readReasoning(selectedModel),
    };
  } catch {
    return fallback;
  }
}

export function buildGrokBuildConfig(values: GrokBuildConfigValues): string {
  return updateGrokBuildConfig(undefined, values);
}

export function updateGrokBuildConfig(
  configToml: string | undefined,
  values: GrokBuildConfigValues,
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
  const modelsTable: Record<string, unknown> = {
    ...existingModels,
    default: profile,
  };
  if (values.reasoningLevels) {
    if (values.defaultReasoningLevel?.trim()) {
      modelsTable.default_reasoning_effort =
        values.defaultReasoningLevel.trim();
    } else {
      delete modelsTable.default_reasoning_effort;
    }
  }
  config.models = modelsTable;

  const modelTables = asRecord(config.model) ?? {};
  const contextWindow =
    Number.isInteger(values.contextWindow) && values.contextWindow > 0
      ? values.contextWindow
      : GROK_BUILD_DEFAULT_CONTEXT_WINDOW;
  const apiBackend = values.apiBackend.trim() || GROK_BUILD_DEFAULT_API_BACKEND;

  const writeProfile = (
    profileKey: string,
    previousKey: string | undefined,
    fields: {
      model: string;
      name: string;
      contextWindow: number;
      reasoningLevels?: string[];
      defaultReasoningLevel?: string;
    },
  ) => {
    const existing =
      asRecord(modelTables[profileKey]) ??
      (previousKey ? asRecord(modelTables[previousKey]) : undefined) ??
      {};
    const apiKey = values.apiKey.trim();
    const envKey = values.envKey?.trim() || asString(existing.env_key).trim();
    const updated: Record<string, unknown> = {
      ...existing,
      model: fields.model,
      base_url: values.baseUrl.trim(),
      name: fields.name,
      api_backend: apiBackend,
      context_window:
        Number.isInteger(fields.contextWindow) && fields.contextWindow > 0
          ? fields.contextWindow
          : contextWindow,
    };
    if (apiKey) updated.api_key = apiKey;
    else delete updated.api_key;
    if (envKey) updated.env_key = envKey;
    else delete updated.env_key;
    if (fields.reasoningLevels) {
      const efforts = reasoningEfforts(
        fields.reasoningLevels,
        fields.defaultReasoningLevel,
      );
      if (efforts) updated.reasoning_efforts = efforts;
      else delete updated.reasoning_efforts;
    }
    return updated;
  };

  if (values.extraModels) {
    const nextTables: Record<string, unknown> = {};
    const used = new Set<string>();
    const writeUnique = (
      profileKey: string,
      previousKey: string | undefined,
      fields: Parameters<typeof writeProfile>[2],
    ) => {
      const key = profileKey.trim();
      if (!key || used.has(key) || !fields.model.trim()) return;
      used.add(key);
      nextTables[key] = writeProfile(key, previousKey, fields);
    };
    writeUnique(profile, previousProfile, {
      model: upstreamModel,
      name: values.name.trim(),
      contextWindow,
      reasoningLevels: values.reasoningLevels,
      defaultReasoningLevel: values.defaultReasoningLevel,
    });
    for (const extra of values.extraModels) {
      const model = extra.model.trim();
      const key = extra.profile?.trim() || model;
      writeUnique(key, undefined, {
        model,
        name: extra.name?.trim() || model,
        contextWindow: extra.contextWindow ?? contextWindow,
        reasoningLevels: extra.reasoningLevels ?? [],
        defaultReasoningLevel: extra.defaultReasoningLevel,
      });
    }
    config.model = nextTables;
  } else {
    const updatedSelected = writeProfile(profile, previousProfile, {
      model: upstreamModel,
      name: values.name.trim(),
      contextWindow,
      reasoningLevels: values.reasoningLevels,
      defaultReasoningLevel: values.defaultReasoningLevel,
    });
    config.model = {
      ...modelTables,
      [profile]: updatedSelected,
    };
    if (previousProfile !== profile && previousProfile in modelTables) {
      delete (config.model as Record<string, unknown>)[previousProfile];
    }
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
    for (const field of ["model", "base_url", "name", "api_backend"]) {
      if (!asString(selected[field]).trim()) return `Missing ${field}`;
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
