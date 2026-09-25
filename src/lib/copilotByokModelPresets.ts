import type {
  CopilotByokApiType,
  CopilotByokModel,
} from "@/lib/api/copilotByok";
import type { ModelsDevModel, ModelsDevResponse } from "@/lib/modelsDevPricing";

export interface CopilotByokModelPresetContext {
  providerName: string;
  url: string;
  apiType: CopilotByokApiType;
}

export interface CopilotByokModelPresetSource {
  modelName?: string | null;
  modelsDev?: ModelsDevResponse | null;
}

export type CopilotByokModelPreset = Partial<
  Pick<
    CopilotByokModel,
    | "toolCalling"
    | "vision"
    | "thinking"
    | "streaming"
    | "contextWindow"
    | "maxInputTokens"
    | "maxOutputTokens"
    | "editTools"
    | "supportsReasoningEffort"
    | "reasoningEffortFormat"
    | "modelOptions"
  >
>;

interface ModelsDevMatch {
  providerId: string;
  providerName: string;
  modelId: string;
  model: ModelsDevModel;
}

function objectValue(value: unknown): Record<string, unknown> {
  return value && !Array.isArray(value) && typeof value === "object"
    ? (value as Record<string, unknown>)
    : {};
}

function endpointHostname(value: string): string {
  try {
    return new URL(value).hostname.toLowerCase();
  } catch {
    return "";
  }
}

function normalizedWords(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
    .trim();
}

function modelIdVariants(value: string): Set<string> {
  const normalized = value.trim().toLowerCase().replace(/\\/g, "/");
  const variants = new Set<string>();
  const add = (candidate: string) => {
    const trimmed = candidate.trim();
    if (!trimmed) return;
    variants.add(trimmed);
    variants.add(trimmed.replace(/@/g, "-"));
    variants.add(trimmed.replace(/\[1m\]$/i, "").trim());
  };
  add(normalized);
  add(normalized.split("/").filter(Boolean).at(-1) ?? "");
  const withoutVersion = normalized.replace(/:\d+$/, "");
  add(withoutVersion);
  add(withoutVersion.split("/").filter(Boolean).at(-1) ?? "");
  return variants;
}

function looseModelIdentity(value: string): string {
  return (
    value
      .trim()
      .toLowerCase()
      .replace(/high[\s_-]*speed/g, "highspeed")
      .replace(/coding/g, "code")
      .replace(/\b(?:kimi|moonshot|mini\s*max|model)\b/g, " ")
      // Coding-plan aliases such as k3-256k describe an endpoint limit, not a
      // distinct model family. Remove only K-token qualifiers; parameter sizes
      // such as 32b and dates remain part of the identity.
      .replace(/\b\d{2,4}k\b/g, " ")
      .replace(/[^a-z0-9.]+/g, "-")
      .replace(/^-+|-+$/g, "")
  );
}

const HOST_PROVIDER_HINTS: Array<[RegExp, readonly string[]]> = [
  [/(?:^|\.)api\.kimi\.com$|(?:^|\.)api\.moonshot\.(?:cn|ai)$/, ["moonshotai"]],
  [/(?:^|\.)api\.minimaxi\.com$/, ["minimax-cn", "minimax"]],
  [/(?:^|\.)api\.minimax\.io$/, ["minimax", "minimax-cn"]],
  [/(?:^|\.)api\.openai\.com$/, ["openai"]],
  [/(?:^|\.)api\.anthropic\.com$/, ["anthropic"]],
  [/(?:^|\.)generativelanguage\.googleapis\.com$/, ["google"]],
  [/(?:^|\.)api\.x\.ai$/, ["xai"]],
  [/(?:^|\.)api\.deepseek\.com$/, ["deepseek"]],
  [/(?:^|\.)(?:open\.)?bigmodel\.cn$|(?:^|\.)api\.z\.ai$/, ["zai"]],
  [/(?:^|\.)dashscope\.aliyuncs\.com$/, ["alibaba"]],
  [/(?:^|\.)openrouter\.ai$/, ["openrouter"]],
];

function familyProviderHints(modelId: string, modelName: string): Set<string> {
  const source = `${modelId} ${modelName}`.trim().toLowerCase();
  const hints = new Set<string>();
  const namespace = modelId.trim().toLowerCase().split("/")[0];
  if (namespace && namespace !== modelId.trim().toLowerCase()) {
    hints.add(namespace);
  }
  const rules: Array<[RegExp, readonly string[]]> = [
    [/\bclaude[-\s]/, ["anthropic"]],
    [/\b(?:gpt|o1|o3|o4)[-.\s]/, ["openai"]],
    [/\bgemini[-.\s]/, ["google"]],
    [/\bgrok[-.\s]/, ["xai"]],
    [/\bdeepseek[-.\s]/, ["deepseek"]],
    [/\bqwen[-.\s\d]/, ["alibaba"]],
    [/\bglm[-.\s\d]/, ["zai"]],
    [/\b(?:kimi|k3|k2(?:\.\d+)?)\b/, ["moonshotai"]],
    [/\bminimax[-.\s]/, ["minimax", "minimax-cn"]],
    [/\bmimo[-.\s]/, ["xiaomi"]],
    [/\blongcat[-.\s]/, ["longcat"]],
  ];
  for (const [pattern, providerIds] of rules) {
    if (pattern.test(source)) providerIds.forEach((id) => hints.add(id));
  }
  return hints;
}

function providerAffinity(
  context: CopilotByokModelPresetContext,
  modelId: string,
  modelName: string,
  providerId: string,
  providerName: string,
): number {
  const normalizedProviderId = providerId.toLowerCase();
  const hostname = endpointHostname(context.url);
  let score = 0;
  for (const [pattern, providerIds] of HOST_PROVIDER_HINTS) {
    if (pattern.test(hostname) && providerIds.includes(normalizedProviderId)) {
      score = Math.max(score, 120 - providerIds.indexOf(normalizedProviderId));
    }
  }

  const namespace = modelId.trim().toLowerCase().split("/")[0];
  if (namespace === normalizedProviderId) score = Math.max(score, 110);
  if (familyProviderHints(modelId, modelName).has(normalizedProviderId)) {
    score = Math.max(score, 80);
  }

  const contextName = normalizedWords(context.providerName);
  const catalogId = normalizedWords(providerId);
  const catalogName = normalizedWords(providerName);
  if (
    contextName &&
    ((catalogId && contextName.includes(catalogId)) ||
      (catalogName &&
        (contextName.includes(catalogName) ||
          catalogName.includes(contextName))))
  ) {
    score = Math.max(score, 100);
  }
  return score;
}

function modelAffinity(
  requestedId: string,
  requestedName: string,
  catalogId: string,
  catalogName: string,
): number {
  const requestedVariants = modelIdVariants(requestedId);
  const catalogVariants = modelIdVariants(catalogId);
  if ([...requestedVariants].some((value) => catalogVariants.has(value))) {
    return 500;
  }

  const looseRequestedName = looseModelIdentity(requestedName);
  const looseCatalogName = looseModelIdentity(catalogName);
  if (
    looseRequestedName &&
    looseCatalogName &&
    looseRequestedName === looseCatalogName
  ) {
    return 450;
  }

  const looseRequestedId = looseModelIdentity(requestedId);
  const looseCatalogId = looseModelIdentity(catalogId);
  if (
    looseRequestedId &&
    looseCatalogId &&
    looseRequestedId === looseCatalogId
  ) {
    return 425;
  }
  return 0;
}

function capabilitySignature(match: ModelsDevMatch): string {
  const model = match.model;
  return JSON.stringify({
    toolCall: model.tool_call,
    reasoning: model.reasoning,
    modalities: model.modalities,
    limit: model.limit,
  });
}

function resolveModelsDevModel(
  context: CopilotByokModelPresetContext,
  modelId: string,
  modelName: string,
  data?: ModelsDevResponse | null,
): ModelsDevMatch | null {
  if (!data) return null;
  const candidates: Array<
    ModelsDevMatch & { score: number; modelScore: number }
  > = [];
  for (const [providerId, provider] of Object.entries(data)) {
    if (!provider || typeof provider !== "object") continue;
    for (const [catalogModelId, model] of Object.entries(
      provider.models ?? {},
    )) {
      const modelScore = modelAffinity(
        modelId,
        modelName,
        catalogModelId,
        model?.name || catalogModelId,
      );
      if (modelScore === 0) continue;
      const affinity = providerAffinity(
        context,
        modelId,
        modelName,
        providerId,
        provider.name || providerId,
      );
      candidates.push({
        providerId,
        providerName: provider.name || providerId,
        modelId: catalogModelId,
        model,
        modelScore,
        score: modelScore + affinity,
      });
    }
  }
  candidates.sort(
    (a, b) =>
      b.score - a.score ||
      b.modelScore - a.modelScore ||
      a.providerId.localeCompare(b.providerId),
  );
  const best = candidates[0];
  if (!best) return null;

  // If neither endpoint/provider nor model family can distinguish two hosts,
  // accept the match only when their capability metadata agrees. This keeps a
  // custom aggregator from silently inheriting another host's smaller limits.
  const tied = candidates.filter((candidate) => candidate.score === best.score);
  if (
    tied.length > 1 &&
    new Set(tied.map((candidate) => capabilitySignature(candidate))).size > 1
  ) {
    return null;
  }
  return best;
}

function positiveInteger(value: unknown): number | undefined {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0
    ? value
    : undefined;
}

function presetFromModelsDev(model: ModelsDevModel): CopilotByokModelPreset {
  const inputModalities = model.modalities?.input
    ?.filter((value): value is string => typeof value === "string")
    .map((value) => value.toLowerCase());
  const preset: CopilotByokModelPreset = {};
  if (typeof model.tool_call === "boolean") {
    preset.toolCalling = model.tool_call;
  }
  if (typeof model.reasoning === "boolean") {
    preset.thinking = model.reasoning;
  }
  if (inputModalities?.length) {
    preset.vision = inputModalities.includes("image");
  } else if (typeof model.attachment === "boolean") {
    preset.vision = model.attachment;
  }
  const contextWindow = positiveInteger(model.limit?.context);
  const maxInputTokens = positiveInteger(model.limit?.input);
  const maxOutputTokens = positiveInteger(model.limit?.output);
  if (contextWindow) preset.contextWindow = contextWindow;
  if (maxInputTokens) preset.maxInputTokens = maxInputTokens;
  if (maxOutputTokens) preset.maxOutputTokens = maxOutputTokens;
  return preset;
}

type KimiModelFamily = "k3" | "k2.7" | "k2.6" | "k2.5";

function kimiModelFamily(
  ...values: Array<string | null | undefined>
): KimiModelFamily | null {
  const source = values.filter(Boolean).join(" ").toLowerCase();
  if (/(?:^|[^a-z0-9])(?:kimi-)?k3(?:[^a-z0-9]|$)/.test(source)) return "k3";
  if (source.includes("k2.7") || source.includes("kimi-for-coding"))
    return "k2.7";
  if (source.includes("k2.6")) return "k2.6";
  if (source.includes("k2.5")) return "k2.5";
  return null;
}

function kimiContractPreset(
  context: CopilotByokModelPresetContext,
  modelId: string,
): CopilotByokModelPreset | null {
  const family = kimiModelFamily(modelId);
  if (!family) return null;
  const base: CopilotByokModelPreset = {
    toolCalling: true,
    vision: true,
    thinking: true,
    streaming: true,
  };
  if (family === "k3") {
    return {
      ...base,
      contextWindow: /(?:^|[^a-z0-9])256k(?:[^a-z0-9]|$)/i.test(modelId)
        ? 262_144
        : 1_000_000,
      supportsReasoningEffort: ["low", "high", "max"],
      reasoningEffortFormat: context.apiType,
      modelOptions: { temperature: 1, top_p: 0.95 },
    };
  }
  if (family === "k2.7") {
    return {
      ...base,
      contextWindow: 262_144,
      maxOutputTokens: 262_144,
      modelOptions: { temperature: 1, top_p: 0.95 },
    };
  }
  return {
    ...base,
    contextWindow: 262_144,
  };
}

function miniMaxModelFamily(modelId: string): "m3" | "m2" | null {
  const normalized =
    modelId.trim().toLowerCase().split(/[/:]/).filter(Boolean).at(-1) ?? "";
  if (/^minimax-m3(?:[-.].*)?$/.test(normalized)) return "m3";
  if (/^minimax-m2(?:[.-].*)?$/.test(normalized)) return "m2";
  return null;
}

function miniMaxContractPreset(modelId: string): CopilotByokModelPreset | null {
  const family = miniMaxModelFamily(modelId);
  if (family === "m3") {
    return {
      toolCalling: true,
      vision: true,
      thinking: true,
      streaming: true,
      contextWindow: 1_000_000,
      maxOutputTokens: 524_288,
      modelOptions: { temperature: 1, top_p: 0.95 },
    };
  }
  if (family === "m2") {
    return {
      toolCalling: true,
      vision: false,
      thinking: true,
      streaming: true,
      contextWindow: 204_800,
      maxOutputTokens: 204_800,
      modelOptions: { temperature: 1, top_p: 0.9 },
    };
  }
  return null;
}

function mergePresets(
  base: CopilotByokModelPreset | null,
  override: CopilotByokModelPreset | null,
): CopilotByokModelPreset | null {
  if (!base) return override;
  if (!override) return base;
  return {
    ...base,
    ...override,
    modelOptions:
      base.modelOptions || override.modelOptions
        ? {
            ...objectValue(base.modelOptions),
            ...objectValue(override.modelOptions),
          }
        : undefined,
  };
}

/**
 * Resolve model capabilities in layers:
 * 1. models.dev metadata (the same model database used by OpenCode), covering
 *    common providers without a growing per-model allowlist;
 * 2. small model-id contract rules for API details models.dev cannot express,
 *    such as Kimi's fixed sampling values and MiniMax output limits.
 *
 * Tool calling defaults on when no catalog or contract says otherwise. VS Code
 * omits models without that capability from agent-mode selection, so surfacing
 * an unknown model is safer than silently hiding it; users can still disable
 * the capability when the endpoint reports that it is unsupported.
 */
export function getCopilotByokModelPreset(
  context: CopilotByokModelPresetContext,
  modelId: string,
  source: CopilotByokModelPresetSource = {},
): CopilotByokModelPreset {
  const modelName = source.modelName?.trim() || modelId;
  const catalogMatch = resolveModelsDevModel(
    context,
    modelId,
    modelName,
    source.modelsDev,
  );
  const catalogPreset = catalogMatch
    ? presetFromModelsDev(catalogMatch.model)
    : null;
  const contractPreset = mergePresets(
    kimiContractPreset(context, modelId),
    miniMaxContractPreset(modelId),
  );
  return {
    toolCalling: true,
    ...(mergePresets(catalogPreset, contractPreset) ?? {}),
  };
}

function hasFixedKimiSampling(modelId: string): boolean {
  const family = kimiModelFamily(modelId);
  return family === "k3" || family === "k2.7";
}

export function mergeCopilotByokModelOptions(
  modelId: string,
  current: unknown,
  preset: unknown,
): Record<string, unknown> {
  const currentOptions = objectValue(current);
  const presetOptions = objectValue(preset);
  // K3 and K2.7 reject VS Code's generic top_p=1 default, so the documented
  // fixed sampling values must override stale/manual values. Other models
  // retain explicit user overrides over catalog defaults.
  return hasFixedKimiSampling(modelId)
    ? { ...currentOptions, ...presetOptions }
    : { ...presetOptions, ...currentOptions };
}
