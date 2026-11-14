import { piProviderPresets, type PiProviderPreset } from "./piProviderPresets";

/**
 * Oh My Pi provider catalog, derived from the Pi catalog.
 *
 * omp validates models.yml against its own schema, so each Pi preset drops what omp doesn't
 * define: the provider-level `name`, the model-level `thinkingLevelMap` (and the catalog
 * reference symbol), and the Pi-only `compat` flags. Pi's DeepSeek thinking dialect and its
 * reasoning-replay flag are translated to omp's equivalents.
 */
export type OmpProviderPreset = Omit<PiProviderPreset, "settingsConfig"> & {
  settingsConfig: Omit<
    PiProviderPreset["settingsConfig"],
    "name" | "models"
  > & {
    models: Record<string, unknown>[];
  };
};

/** Pi `compat` keys that omp's schema does not define. */
const PI_ONLY_COMPAT_KEYS = new Set([
  "forceAdaptiveThinking",
  "requiresReasoningContentOnAssistantMessages",
  "deferredToolsMode",
]);

function toOmpCompat(compat: Record<string, unknown>): Record<string, unknown> {
  const result = Object.fromEntries(
    Object.entries(compat).filter(([key]) => !PI_ONLY_COMPAT_KEYS.has(key)),
  );
  // Pi's DeepSeek dialect and omp's zai dialect both send thinking.type.
  if (result.thinkingFormat === "deepseek") result.thinkingFormat = "zai";
  if (compat.requiresReasoningContentOnAssistantMessages === true) {
    result.reasoningContentField = "reasoning_content";
    result.requiresReasoningContentForToolCalls = true;
    result.requiresReasoningContentForAllAssistantTurns = true;
    result.allowsSyntheticReasoningContentForToolCalls = false;
  }
  return result;
}

function toOmpModel(
  model: PiProviderPreset["settingsConfig"]["models"][number],
): Record<string, unknown> {
  // Spreading drops the catalog reference symbol; thinkingLevelMap is Pi-only.
  const { thinkingLevelMap: _piOnly, compat, ...rest } = model;
  const ompCompat = toOmpCompat(compat ?? {});
  return Object.keys(ompCompat).length > 0
    ? { ...rest, compat: ompCompat }
    : rest;
}

function toOmpPreset(preset: PiProviderPreset): OmpProviderPreset {
  const { name: _piOnly, compat, models, ...config } = preset.settingsConfig;
  return {
    ...preset,
    settingsConfig: {
      ...config,
      ...(compat !== undefined ? { compat: toOmpCompat(compat) } : {}),
      models: models.map(toOmpModel),
    },
  };
}

export const ompProviderPresets: OmpProviderPreset[] =
  piProviderPresets.map(toOmpPreset);
