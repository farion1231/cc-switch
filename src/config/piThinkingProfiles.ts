import type { PiModelCatalogKey } from "./piModelCatalog";
import type { PiApiFormat } from "./piProviderPresets";

export const PI_THINKING_LEVELS = [
  "off",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
] as const;

export type PiThinkingLevel = (typeof PI_THINKING_LEVELS)[number];
export type PiThinkingLevelMap = Partial<
  Record<PiThinkingLevel, string | null>
>;

interface PiThinkingProfile {
  map: Readonly<PiThinkingLevelMap>;
}

/**
 * Small, reviewed Pi-native maps.
 *
 * Missing keys and null values are intentionally different. An empty profile
 * is forbidden because `{}` is reserved for a user's explicit "use Pi
 * defaults" choice.
 */
export const piThinkingProfiles = {
  xhighAndMax: {
    map: {
      xhigh: "xhigh",
      max: "max",
    },
  },
  deepseekV4: {
    map: {
      minimal: null,
      low: null,
      medium: null,
      high: "high",
      max: "max",
    },
  },
  offUnsupported: {
    map: {
      off: null,
    },
  },
  kimi3: {
    map: {
      off: null,
      minimal: null,
      low: "low",
      medium: null,
      high: "high",
      xhigh: null,
      max: "max",
    },
  },
  openCodeGoGlm52: {
    map: {
      off: null,
      minimal: null,
      low: null,
      medium: null,
      high: "high",
      xhigh: null,
      max: "max",
    },
  },
  offUnsupportedXhighAndMax: {
    map: {
      off: null,
      xhigh: "xhigh",
      max: "max",
    },
  },
  maxOnly: {
    map: {
      max: "max",
    },
  },
  lowMediumHighOnly: {
    map: {
      off: null,
      minimal: null,
      low: "low",
      medium: "medium",
      high: "high",
      xhigh: null,
      max: null,
    },
  },
  openaiResponsesGpt5: {
    map: {
      off: null,
      minimal: "minimal",
      low: "low",
      medium: "medium",
      high: "high",
      xhigh: null,
      max: null,
    },
  },
  openaiResponsesGpt51: {
    map: {
      off: "none",
      minimal: null,
      low: "low",
      medium: "medium",
      high: "high",
      xhigh: null,
      max: null,
    },
  },
  openaiResponsesGpt52To55: {
    map: {
      off: "none",
      minimal: null,
      low: "low",
      medium: "medium",
      high: "high",
      xhigh: "xhigh",
      max: null,
    },
  },
  openaiResponsesGpt53CodexSpark: {
    map: {
      off: null,
      minimal: null,
      low: "low",
      medium: "medium",
      high: "high",
      xhigh: "xhigh",
      max: null,
    },
  },
  openaiResponsesGpt56: {
    map: {
      off: "none",
      minimal: null,
      low: "low",
      medium: "medium",
      high: "high",
      xhigh: "xhigh",
      max: "max",
    },
  },
  geminiLowHigh: {
    map: {
      off: null,
      minimal: null,
      low: "LOW",
      medium: null,
      high: "HIGH",
    },
  },
} as const satisfies Record<string, PiThinkingProfile>;

export type PiThinkingProfileId = keyof typeof piThinkingProfiles;

export interface PiThinkingBinding {
  catalogKey: PiModelCatalogKey;
  api: PiApiFormat;
  profileId: PiThinkingProfileId;
  modelCompat?: {
    forceAdaptiveThinking: true;
  };
}

/**
 * Generic bindings use a reviewed logical model identity and provider-level
 * API. Host-sensitive models opt in from a preset instead. Values are kept
 * only when current Pi and independent model/provider references agree on the
 * transport semantics.
 */
export const piThinkingBindings: readonly PiThinkingBinding[] = [
  {
    catalogKey: "anthropic/claude-fable-5",
    api: "anthropic-messages",
    profileId: "offUnsupportedXhighAndMax",
    modelCompat: { forceAdaptiveThinking: true },
  },
  {
    catalogKey: "anthropic/claude-opus-4.6",
    api: "anthropic-messages",
    profileId: "maxOnly",
    modelCompat: { forceAdaptiveThinking: true },
  },
  {
    catalogKey: "anthropic/claude-opus-4.7",
    api: "anthropic-messages",
    profileId: "xhighAndMax",
    modelCompat: { forceAdaptiveThinking: true },
  },
  {
    catalogKey: "anthropic/claude-opus-4.8",
    api: "anthropic-messages",
    profileId: "xhighAndMax",
    modelCompat: { forceAdaptiveThinking: true },
  },
  {
    catalogKey: "anthropic/claude-opus-5",
    api: "anthropic-messages",
    profileId: "xhighAndMax",
    modelCompat: { forceAdaptiveThinking: true },
  },
  {
    catalogKey: "anthropic/claude-sonnet-5",
    api: "anthropic-messages",
    profileId: "xhighAndMax",
    modelCompat: { forceAdaptiveThinking: true },
  },
  {
    catalogKey: "google/gemini-3.1-pro-preview",
    api: "google-generative-ai",
    profileId: "geminiLowHigh",
  },
  {
    catalogKey: "google/gemini-3.5-flash",
    api: "google-generative-ai",
    profileId: "offUnsupported",
  },
  {
    catalogKey: "google/gemini-3.6-flash",
    api: "google-generative-ai",
    profileId: "offUnsupported",
  },
  {
    catalogKey: "openai/gpt-5",
    api: "openai-responses",
    profileId: "openaiResponsesGpt5",
  },
  {
    catalogKey: "openai/gpt-5-mini",
    api: "openai-responses",
    profileId: "openaiResponsesGpt5",
  },
  {
    catalogKey: "openai/gpt-5.1",
    api: "openai-responses",
    profileId: "openaiResponsesGpt51",
  },
  {
    catalogKey: "openai/gpt-5.2",
    api: "openai-responses",
    profileId: "openaiResponsesGpt52To55",
  },
  {
    catalogKey: "openai/gpt-5.3-codex",
    api: "openai-responses",
    profileId: "openaiResponsesGpt52To55",
  },
  {
    catalogKey: "openai/gpt-5.3-codex-spark",
    api: "openai-responses",
    profileId: "openaiResponsesGpt53CodexSpark",
  },
  {
    catalogKey: "openai/gpt-5.4",
    api: "openai-responses",
    profileId: "openaiResponsesGpt52To55",
  },
  {
    catalogKey: "openai/gpt-5.4-mini",
    api: "openai-responses",
    profileId: "openaiResponsesGpt52To55",
  },
  {
    catalogKey: "openai/gpt-5.5",
    api: "openai-responses",
    profileId: "openaiResponsesGpt52To55",
  },
  {
    catalogKey: "openai/gpt-5.6-luna",
    api: "openai-responses",
    profileId: "openaiResponsesGpt56",
  },
  {
    catalogKey: "openai/gpt-5.6-sol",
    api: "openai-responses",
    profileId: "openaiResponsesGpt56",
  },
  {
    catalogKey: "openai/gpt-5.6-terra",
    api: "openai-responses",
    profileId: "openaiResponsesGpt56",
  },
  {
    catalogKey: "openai/o3",
    api: "openai-responses",
    profileId: "lowMediumHighOnly",
  },
  {
    catalogKey: "openai/o4-mini",
    api: "openai-responses",
    profileId: "lowMediumHighOnly",
  },
  {
    catalogKey: "xai/grok-4.5",
    api: "openai-responses",
    profileId: "lowMediumHighOnly",
  },
];

export interface ResolvedPiThinkingProfile {
  profileId: PiThinkingProfileId;
  map: PiThinkingLevelMap;
  modelCompat?: {
    forceAdaptiveThinking: true;
  };
}

export function clonePiThinkingLevelMap(
  map: Readonly<PiThinkingLevelMap>,
): PiThinkingLevelMap {
  return { ...map };
}

export function getPiThinkingProfile(
  profileId: PiThinkingProfileId,
): ResolvedPiThinkingProfile {
  return {
    profileId,
    map: clonePiThinkingLevelMap(piThinkingProfiles[profileId].map),
  };
}

export function resolvePiThinkingProfile(options: {
  catalogKey: PiModelCatalogKey;
  api?: string | null;
}): ResolvedPiThinkingProfile | undefined {
  const binding = (piThinkingBindings as readonly PiThinkingBinding[]).find(
    (candidate) =>
      candidate.catalogKey === options.catalogKey &&
      candidate.api === options.api,
  );
  if (!binding) return undefined;
  return {
    ...getPiThinkingProfile(binding.profileId),
    ...(binding.modelCompat ? { modelCompat: { ...binding.modelCompat } } : {}),
  };
}

export function isPiThinkingLevelMap(
  value: unknown,
): value is PiThinkingLevelMap {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const allowed = new Set<string>(PI_THINKING_LEVELS);
  return Object.entries(value).every(
    ([key, entry]) =>
      allowed.has(key) && (typeof entry === "string" || entry === null),
  );
}
