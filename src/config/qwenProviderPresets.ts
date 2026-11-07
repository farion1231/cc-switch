import { z } from "zod";
import {
  piProviderPresets,
  type PiApiFormat,
  type PiPresetModel,
  type PiProviderPreset,
} from "./piProviderPresets";

/** Qwen Code `modelProviders` keys (`security.auth.selectedType` values). */
export const QWEN_AUTH_TYPES = [
  "openai",
  "openai-responses",
  "anthropic",
  "gemini",
  "vertex-ai",
] as const;

/**
 * Protocols offered when creating a provider. `gemini` and `vertex-ai` are kept
 * only for entries imported from settings.json: OpenAI-compatible relays rarely
 * serve the Gemini `/v1beta` API that Qwen Code's Gemini client calls.
 */
export const QWEN_EDITABLE_AUTH_TYPES = [
  "openai",
  "openai-responses",
  "anthropic",
] as const;

const qwenAuthTypeSchema = z.enum(QWEN_AUTH_TYPES);

// Unknown native fields (description, capabilities, wireApi, samplingParams…)
// belong to Qwen Code and must survive edits unchanged.
export const qwenModelSchema = z
  .object({
    id: z.string(),
    name: z.string().optional(),
    authType: qwenAuthTypeSchema.optional(),
    baseUrl: z.string().optional(),
    generationConfig: z
      .object({ contextWindowSize: z.number().optional() })
      .passthrough()
      .optional(),
  })
  .passthrough();

export const qwenConfigSchema = z
  .object({
    authType: qwenAuthTypeSchema,
    baseUrl: z.string().optional(),
    apiKey: z.string().optional(),
    envKey: z.string().optional(),
    models: z.array(qwenModelSchema),
  })
  .passthrough();

export type QwenAuthType = z.infer<typeof qwenAuthTypeSchema>;
export type QwenModel = z.infer<typeof qwenModelSchema>;
export type QwenProviderConfig = z.infer<typeof qwenConfigSchema>;

export function emptyQwenConfig(): QwenProviderConfig {
  return { authType: "openai", baseUrl: "", apiKey: "", models: [] };
}

export interface QwenProviderPreset
  extends Omit<PiProviderPreset, "settingsConfig"> {
  settingsConfig: QwenProviderConfig;
}

// Only native protocol equivalents are admitted. Bedrock's signed Converse API
// has no equivalent in Qwen Code and must not become an OpenAI/Anthropic preset;
// Gemini presets are not offered (see QWEN_EDITABLE_AUTH_TYPES).
const QWEN_API_FORMATS: Partial<Record<PiApiFormat, QwenAuthType>> = {
  "openai-completions": "openai",
  "openai-responses": "openai-responses",
  "anthropic-messages": "anthropic",
};

function toQwenModel(
  model: PiPresetModel,
  headers: Record<string, string> | undefined,
): QwenModel {
  return {
    id: model.id,
    name: model.name,
    generationConfig: {
      contextWindowSize: model.contextWindow,
      ...(headers ? { customHeaders: { ...headers } } : {}),
    },
  };
}

// Share the reviewed supplier catalog and grouping metadata with Pi; translate
// connection and model fields explicitly, without leaking Pi-only SDK options.
export const qwenProviderPresets: QwenProviderPreset[] =
  piProviderPresets.flatMap((preset) => {
    const { settingsConfig: source, ...identity } = preset;
    const authType = QWEN_API_FORMATS[source.api];
    if (!authType || source.compat || !source.models.length) return [];
    return [
      {
        ...identity,
        providerKey: preset.providerKey.replace(/^cc-switch-/, ""),
        settingsConfig: {
          authType,
          baseUrl: source.baseUrl,
          apiKey: source.apiKey || (preset.category === "local" ? "local" : ""),
          models: source.models.map((model) =>
            toQwenModel(model, source.headers),
          ),
        },
      },
    ];
  });
