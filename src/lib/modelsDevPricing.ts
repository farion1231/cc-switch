import type { ModelPricing, ModelsDevSyncConfig } from "@/types/usage";
import {
  normalizeModelsDevModelId,
  type ModelsDevModel,
  type ModelsDevResponse,
} from "./modelsDev";

export interface ModelsDevEntry {
  key: string;
  providerId: string;
  providerName: string;
  modelId: string;
  normalizedId: string;
  modelName: string;
  releaseDate: string;
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
}

const NON_TEXT_MODEL_MARKERS = [
  "audio",
  "deprecated",
  "embedding",
  "image",
  "moderation",
  "realtime",
  "transcribe",
  "tts",
  "video",
];
const NON_TEXT_OUTPUT_MODALITIES = new Set(["audio", "image", "video"]);

const isTextPricingModel = (modelId: string, model?: ModelsDevModel) => {
  if (model?.status?.toLowerCase() === "deprecated") return false;

  const outputModalities = model?.modalities?.output
    ?.filter((modality): modality is string => typeof modality === "string")
    .map((modality) => modality.toLowerCase());
  if (
    outputModalities?.length &&
    (!outputModalities.includes("text") ||
      outputModalities.some((modality) =>
        NON_TEXT_OUTPUT_MODALITIES.has(modality),
      ))
  ) {
    return false;
  }

  const searchableName = `${modelId} ${model?.name ?? ""}`.toLowerCase();
  return !NON_TEXT_MODEL_MARKERS.some((marker) =>
    searchableName.includes(marker),
  );
};

export function formatPrice(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "0";
  if (value >= 1e12) return "0";
  const trimmed = value.toFixed(6).replace(/0+$/, "").replace(/\.$/, "");
  return trimmed || "0";
}

export function flattenModels(data: ModelsDevResponse): ModelsDevEntry[] {
  const entries: ModelsDevEntry[] = [];
  for (const [providerId, provider] of Object.entries(data)) {
    if (!provider || typeof provider !== "object") continue;
    const providerName = provider.name || providerId;
    for (const [modelId, model] of Object.entries(provider.models ?? {})) {
      if (!isTextPricingModel(modelId, model)) continue;
      const cost = model?.cost;
      const input = typeof cost?.input === "number" ? cost.input : null;
      const output = typeof cost?.output === "number" ? cost.output : null;
      if (input === null && output === null) continue;
      const normalizedId = normalizeModelsDevModelId(modelId);
      if (!normalizedId) continue;
      entries.push({
        key: `${providerId}/${modelId}`,
        providerId,
        providerName,
        modelId,
        normalizedId,
        modelName: model?.name || modelId,
        releaseDate:
          typeof model?.release_date === "string" ? model.release_date : "",
        input: input ?? 0,
        output: output ?? 0,
        cacheRead: typeof cost?.cache_read === "number" ? cost.cache_read : 0,
        cacheWrite:
          typeof cost?.cache_write === "number" ? cost.cache_write : 0,
      });
    }
  }
  entries.sort(
    (a, b) =>
      b.releaseDate.localeCompare(a.releaseDate) ||
      a.modelName.localeCompare(b.modelName),
  );
  return entries;
}

const COMMON_MODEL_LIMIT_PER_FAMILY = 6;

interface CommonFamilyRule {
  id: string;
  providers: ReadonlySet<string>;
  matches: (modelId: string) => boolean;
}

const COMMON_FAMILY_RULES: CommonFamilyRule[] = [
  {
    id: "claude",
    providers: new Set(["anthropic"]),
    matches: (modelId) => modelId.startsWith("claude-"),
  },
  {
    id: "gpt",
    providers: new Set(["openai"]),
    matches: (modelId) =>
      modelId.startsWith("gpt-") ||
      modelId.startsWith("o1-") ||
      modelId.startsWith("o3-") ||
      modelId.startsWith("o4-"),
  },
  {
    id: "gemini",
    providers: new Set(["google"]),
    matches: (modelId) => modelId.startsWith("gemini-"),
  },
  {
    id: "grok",
    providers: new Set(["xai"]),
    matches: (modelId) => modelId.startsWith("grok-"),
  },
  {
    id: "deepseek",
    providers: new Set(["deepseek"]),
    matches: (modelId) => modelId.startsWith("deepseek-"),
  },
  {
    id: "qwen",
    providers: new Set(["alibaba"]),
    matches: (modelId) => modelId.startsWith("qwen"),
  },
  {
    id: "mimo",
    providers: new Set(["xiaomi"]),
    matches: (modelId) => modelId.startsWith("mimo-"),
  },
  {
    id: "longcat",
    providers: new Set(["longcat"]),
    matches: (modelId) => modelId.startsWith("longcat-"),
  },
  {
    id: "kimi",
    providers: new Set(["moonshotai"]),
    matches: (modelId) => modelId.startsWith("kimi-"),
  },
  {
    id: "minimax",
    providers: new Set(["minimax-cn"]),
    matches: (modelId) => modelId.startsWith("minimax-m"),
  },
  {
    id: "glm",
    providers: new Set(["zai"]),
    matches: (modelId) => modelId.startsWith("glm-"),
  },
];

/** Pick a bounded, canonical set of recent chat/coding models per family. */
export function getCommonModelKeys(entries: ModelsDevEntry[]): Set<string> {
  const keys = new Set<string>();
  for (const rule of COMMON_FAMILY_RULES) {
    let count = 0;
    for (const entry of entries) {
      if (
        rule.providers.has(entry.providerId) &&
        rule.matches(entry.modelId.toLowerCase())
      ) {
        keys.add(entry.key);
        count += 1;
        if (count >= COMMON_MODEL_LIMIT_PER_FAMILY) break;
      }
    }
  }
  return keys;
}

export function resolveModelsDevSelection(
  entries: ModelsDevEntry[],
  config: ModelsDevSyncConfig,
): ModelsDevEntry[] {
  const explicit = new Set(config.selectedModelKeys);
  const excluded = new Set(config.excludedCommonModelKeys);
  const common = config.includeCommonModels
    ? getCommonModelKeys(entries)
    : new Set<string>();
  return entries.filter(
    (entry) =>
      explicit.has(entry.key) ||
      (common.has(entry.key) && !excluded.has(entry.key)),
  );
}

export function toModelPricing(entries: ModelsDevEntry[]): ModelPricing[] {
  const byModelId = new Map<string, ModelPricing>();
  for (const entry of entries) {
    if (byModelId.has(entry.normalizedId)) continue;
    byModelId.set(entry.normalizedId, {
      modelId: entry.normalizedId,
      displayName: entry.modelName,
      inputCostPerMillion: formatPrice(entry.input),
      outputCostPerMillion: formatPrice(entry.output),
      cacheReadCostPerMillion: formatPrice(entry.cacheRead),
      cacheCreationCostPerMillion: formatPrice(entry.cacheWrite),
    });
  }
  return Array.from(byModelId.values());
}
