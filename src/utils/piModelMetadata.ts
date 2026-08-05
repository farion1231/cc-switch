import {
  piProviderPresets,
  type PiPresetModel,
  type PiProviderPreset,
} from "@/config/piProviderPresets";
import type { ModelsDevModel, ModelsDevResponse } from "@/lib/modelsDevPricing";

export type PiModelMetadataSource = "preset" | "models-dev";

export interface PiModelMetadata {
  name?: string;
  reasoning?: boolean;
  imageInput?: boolean;
  contextWindow?: number;
  maxTokens?: number;
  sources: PiModelMetadataSource[];
}

interface MetadataCandidate extends Omit<PiModelMetadata, "sources"> {
  providerId?: string;
  providerName?: string;
  source: PiModelMetadataSource;
}

interface ResolvePiModelMetadataOptions {
  selectedPreset?: PiProviderPreset | null;
  modelsDevCatalog?: ModelsDevResponse | null;
  preferredProvider?: string | null;
}

type ComparableMetadataValue = string | number | boolean;

function positiveNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && value > 0
    ? value
    : undefined;
}

function mostCommon<T extends ComparableMetadataValue>(
  values: Array<T | undefined>,
): T | undefined {
  const counts = new Map<T, number>();
  for (const value of values) {
    if (value === undefined) continue;
    counts.set(value, (counts.get(value) ?? 0) + 1);
  }
  const ranked = Array.from(counts.entries()).sort(
    (left, right) =>
      right[1] - left[1] || String(left[0]).localeCompare(String(right[0])),
  );
  if (ranked.length === 0) return undefined;
  if (ranked.length > 1 && ranked[0][1] === ranked[1][1]) return undefined;
  return ranked[0][0];
}

function collapseCandidates(
  candidates: MetadataCandidate[],
): PiModelMetadata | undefined {
  if (candidates.length === 0) return undefined;
  const metadata: PiModelMetadata = {
    name: mostCommon(candidates.map((candidate) => candidate.name)),
    reasoning: mostCommon(candidates.map((candidate) => candidate.reasoning)),
    imageInput: mostCommon(candidates.map((candidate) => candidate.imageInput)),
    contextWindow: mostCommon(
      candidates.map((candidate) => candidate.contextWindow),
    ),
    maxTokens: mostCommon(candidates.map((candidate) => candidate.maxTokens)),
    sources: Array.from(
      new Set(candidates.map((candidate) => candidate.source)),
    ),
  };
  const hasResolvedField = Object.entries(metadata).some(
    ([key, value]) => key !== "sources" && value !== undefined,
  );
  return hasResolvedField ? metadata : undefined;
}

function presetCandidate(model: PiPresetModel): MetadataCandidate {
  return {
    source: "preset",
    name: model.name,
    reasoning:
      typeof model.reasoning === "boolean" ? model.reasoning : undefined,
    imageInput: Array.isArray(model.input)
      ? model.input.includes("image")
      : undefined,
    contextWindow: positiveNumber(model.contextWindow),
    maxTokens: positiveNumber(model.maxTokens),
  };
}

function modelsDevCandidate(
  providerId: string,
  providerName: string,
  model: ModelsDevModel,
): MetadataCandidate {
  const inputModalities = Array.isArray(model.modalities?.input)
    ? model.modalities.input
    : undefined;
  return {
    source: "models-dev",
    providerId,
    providerName,
    name:
      typeof model.name === "string" && model.name.length > 0
        ? model.name
        : undefined,
    reasoning:
      typeof model.reasoning === "boolean" ? model.reasoning : undefined,
    imageInput: inputModalities ? inputModalities.includes("image") : undefined,
    contextWindow: positiveNumber(model.limit?.context),
    maxTokens: positiveNumber(model.limit?.output),
  };
}

function collectPresetCandidates(
  modelId: string,
  selectedPreset?: PiProviderPreset | null,
): MetadataCandidate[] {
  const selectedMatches =
    selectedPreset?.settingsConfig.models
      .filter((model) => model.id === modelId)
      .map(presetCandidate) ?? [];
  if (selectedMatches.length > 0) return selectedMatches;

  return piProviderPresets.flatMap((preset) =>
    preset.settingsConfig.models
      .filter((model) => model.id === modelId)
      .map(presetCandidate),
  );
}

function collectModelsDevCandidates(
  modelId: string,
  catalog?: ModelsDevResponse | null,
  preferredProvider?: string | null,
): MetadataCandidate[] {
  if (!catalog) return [];
  const matches: MetadataCandidate[] = [];
  for (const [providerId, provider] of Object.entries(catalog)) {
    if (!provider || typeof provider !== "object") continue;
    const providerName = provider.name || providerId;
    for (const [catalogModelId, model] of Object.entries(
      provider.models ?? {},
    )) {
      if (
        catalogModelId !== modelId &&
        (typeof model?.id !== "string" || model.id !== modelId)
      ) {
        continue;
      }
      matches.push(modelsDevCandidate(providerId, providerName, model));
    }
  }

  const normalizedPreference = preferredProvider?.trim().toLowerCase();
  if (!normalizedPreference) return matches;
  const preferredMatches = matches.filter(
    (candidate) =>
      candidate.providerId?.toLowerCase() === normalizedPreference ||
      candidate.providerName?.toLowerCase() === normalizedPreference,
  );
  return preferredMatches.length > 0 ? preferredMatches : matches;
}

function mergeMetadata(
  primary?: PiModelMetadata,
  fallback?: PiModelMetadata,
): PiModelMetadata | undefined {
  if (!primary) return fallback;
  if (!fallback) return primary;
  return {
    name: primary.name ?? fallback.name,
    reasoning: primary.reasoning ?? fallback.reasoning,
    imageInput: primary.imageInput ?? fallback.imageInput,
    contextWindow: primary.contextWindow ?? fallback.contextWindow,
    maxTokens: primary.maxTokens ?? fallback.maxTokens,
    sources: Array.from(new Set([...primary.sources, ...fallback.sources])),
  };
}

/**
 * Resolve only exact model IDs. Pi treats IDs as opaque strings, so fuzzy
 * family or protocol guesses could silently assign capabilities to the wrong
 * upstream model.
 */
export function resolvePiModelMetadata(
  modelId: string,
  options: ResolvePiModelMetadataOptions = {},
): PiModelMetadata | undefined {
  if (modelId.length === 0) return undefined;
  const presetMetadata = collapseCandidates(
    collectPresetCandidates(modelId, options.selectedPreset),
  );
  const modelsDevMetadata = collapseCandidates(
    collectModelsDevCandidates(
      modelId,
      options.modelsDevCatalog,
      options.preferredProvider,
    ),
  );
  return mergeMetadata(presetMetadata, modelsDevMetadata);
}
