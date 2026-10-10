import type { PresetTheme } from "./claudeProviderPresets";
import type { PresetFamilyFields } from "./presetFamilies";
import type { PiProviderPreset } from "./piProviderPresets";
import { piProviderPresets } from "./piProviderPresets";
import { PI_MODEL_CATALOG_REFERENCE } from "./piModelCatalog";
import type { ProviderCategory } from "@/types";

/**
 * Oh My Pi provider catalog, derived from the Pi catalog.
 *
 * `models.yml` is validated by Oh My Pi's `ModelsConfigSchema`, and only
 * known fields must follow its native contract. Pi presets therefore translate
 * their thinking dialect and strip Pi-only model controls:
 *
 * - `thinkingLevelMap` (Pi-only model field) and the embedded model-catalog
 *   reference symbol;
 * - the `compat` keys `forceAdaptiveThinking`,
 *   `requiresReasoningContentOnAssistantMessages` and `deferredToolsMode`
 *   (Pi harness compat flags absent from omp's schema) — writing them into
 *   OMP's config would not apply those flags. Reasoning replay is translated
 *   to omp's tool-call replay controls instead.
 *
 * Oh My Pi presets that have no Pi counterpart are appended below so the omp
 * catalog keeps its own entries.
 *
 * Each preset carries its own registry `icon` + `iconColor`, so a provider
 * created from a preset renders its own logo in the provider list (not the
 * agent mark) — matching the other agent pages.
 */

export type OhMyPiApiFormat =
  | "openai-completions"
  | "openai-responses"
  | "anthropic-messages"
  | "google-generative-ai"
  | "bedrock-converse-stream";

export interface OhMyPiPresetModel {
  id: string;
  name: string;
  reasoning: boolean;
  input: ("text" | "image")[];
  contextWindow: number;
  maxTokens: number;
  compat?: Record<string, unknown>;
  headers?: Record<string, string>;
}

export interface OhMyPiProviderPreset extends PresetFamilyFields {
  name: string;
  nameKey?: string;
  providerKey: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: {
    name: string;
    baseUrl: string;
    api: OhMyPiApiFormat;
    apiKey: string;
    headers?: Record<string, string>;
    compat?: Record<string, unknown>;
    models: OhMyPiPresetModel[];
  };
  category?: ProviderCategory;
  isPartner?: boolean;
  primePartner?: boolean;
  partnerPromotionKey?: string;
  theme?: PresetTheme;
  icon?: string;
  iconColor?: string;
}

/** Pi `compat` keys that omp's `ModelsConfigSchema` does not define. */
const OMP_INCOMPATIBLE_COMPAT_KEYS: Record<string, true> = {
  forceAdaptiveThinking: true,
  requiresReasoningContentOnAssistantMessages: true,
  deferredToolsMode: true,
};

interface OhMyPiModelSeed {
  id: string;
  name: string;
  reasoning: boolean;
  input: ("text" | "image")[];
  contextWindow: number;
  maxTokens: number;
  compat?: Record<string, unknown>;
  headers?: Record<string, string>;
}

interface OhMyPiPresetSeed extends PresetFamilyFields {
  name: string;
  nameKey?: string;
  providerKey: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: {
    name: string;
    baseUrl: string;
    api: OhMyPiApiFormat;
    apiKey: string;
    headers?: Record<string, string>;
    compat?: Record<string, unknown>;
    models: OhMyPiModelSeed[];
  };
  category?: ProviderCategory;
  isPartner?: boolean;
  primePartner?: boolean;
  partnerPromotionKey?: string;
  icon?: string;
  iconColor?: string;
}

// Pi's DeepSeek dialect and omp's zai dialect both send thinking.type.
// Omp uses explicit reasoning replay controls instead of Pi's assistant flag.
function toOhMyPiCompat(
  compat: Record<string, unknown>,
): Record<string, unknown> {
  const result = Object.fromEntries(
    Object.entries(compat).filter(
      ([key]) => !(key in OMP_INCOMPATIBLE_COMPAT_KEYS),
    ),
  );
  if (result.thinkingFormat === "deepseek") result.thinkingFormat = "zai";
  if (compat.requiresReasoningContentOnAssistantMessages === true) {
    result.reasoningContentField = "reasoning_content";
    result.requiresReasoningContentForToolCalls = true;
    result.requiresReasoningContentForAllAssistantTurns = true;
    result.allowsSyntheticReasoningContentForToolCalls = false;
  }
  return result;
}

function toOhMyPiModel(model: Record<string, unknown>): OhMyPiModelSeed {
  const seed: OhMyPiModelSeed = {
    id: String(model.id),
    name: String(model.name),
    reasoning: model.reasoning === true,
    input: (Array.isArray(model.input) ? model.input : ["text"]) as (
      | "text"
      | "image"
    )[],
    contextWindow: Number(model.contextWindow),
    maxTokens: Number(model.maxTokens),
  };
  if (model.headers !== undefined) {
    seed.headers = model.headers as Record<string, string>;
  }
  const compat = model.compat as Record<string, unknown> | undefined;
  const stripped = toOhMyPiCompat(compat ?? {});
  if (Object.keys(stripped).length > 0) {
    seed.compat = stripped;
  }
  return seed;
}

function toOhMyPiPreset(preset: PiProviderPreset): OhMyPiPresetSeed {
  const config = preset.settingsConfig;
  return {
    ...preset,
    settingsConfig: {
      name: config.name,
      baseUrl: config.baseUrl,
      api: config.api,
      apiKey: config.apiKey,
      ...(config.headers !== undefined ? { headers: config.headers } : {}),
      ...(config.compat !== undefined
        ? {
            compat: toOhMyPiCompat(config.compat),
          }
        : {}),
      models: config.models
        .filter((model) => model.id !== undefined)
        .map((model) =>
          toOhMyPiModel(
            model as unknown as Record<string, unknown> & {
              [PI_MODEL_CATALOG_REFERENCE]?: unknown;
            },
          ),
        ),
    },
  };
}

/** Oh My Pi presets with no Pi counterpart, kept omp-native. */
const ohmypiOnlyPresetSeeds: OhMyPiPresetSeed[] = [
  {
    name: "ETok.ai",
    providerKey: "cc-switch-etok-ai",
    websiteUrl: "https://etok.ai",
    apiKeyUrl: "https://etok.ai",
    settingsConfig: {
      name: "ETok",
      baseUrl: "https://api.etok.ai",
      api: "anthropic-messages",
      apiKey: "",
      models: [
        {
          id: "claude-opus-5",
          name: "Claude Opus 5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
        {
          id: "claude-sonnet-5",
          name: "Claude Sonnet 5",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 1000000,
          maxTokens: 128000,
        },
      ],
    },
    category: "third_party",
  },
  {
    name: "Bailian",
    providerKey: "cc-switch-bailian",
    websiteUrl: "https://bailian.console.aliyun.com",
    apiKeyUrl: "https://bailian.console.aliyun.com/#/api-key",
    settingsConfig: {
      name: "Bailian",
      baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      api: "openai-completions",
      apiKey: "",
      models: [
        {
          id: "qwen3-coder-plus",
          name: "Qwen3 Coder Plus",
          reasoning: false,
          input: ["text"],
          contextWindow: 1000000,
          maxTokens: 65536,
        },
      ],
    },
    category: "cn_official",
    icon: "bailian",
    iconColor: "#624AFF",
  },
];

export const ohmypiProviderPresets: OhMyPiProviderPreset[] = [
  ...piProviderPresets.map(toOhMyPiPreset),
  ...ohmypiOnlyPresetSeeds,
];
