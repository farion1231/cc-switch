import { z } from "zod";
import {
  piProviderPresets,
  type PiApiFormat,
  type PiPresetModel,
  type PiProviderPreset,
} from "./piProviderPresets";

export const KIMI_PROVIDER_TYPES = [
  "kimi",
  "openai",
  "openai_responses",
  "anthropic",
  "google-genai",
  "vertexai",
] as const;
export const KIMI_CAPABILITIES = [
  "image_in",
  "thinking",
  "tool_use",
  "always_thinking",
  "dynamically_loaded_tools",
] as const;

// The editor only manages KIMI_CAPABILITIES. Other native capabilities belong to
// Kimi and must survive edits unchanged, including names added by future versions.
const kimiCapabilitiesSchema = z.array(z.string());

export const kimiConfigSchema = z
  .object({
    provider: z
      .object({
        type: z.enum(KIMI_PROVIDER_TYPES),
        base_url: z.string().optional(),
        api_key: z.string().optional(),
        api_key_env: z.string().optional(),
        env: z.record(z.string(), z.string()).optional(),
        custom_headers: z.record(z.string(), z.string()).optional(),
        oauth: z
          .object({ key: z.string(), storage: z.string().optional() })
          .passthrough()
          .optional(),
      })
      .passthrough(),
    models: z.record(
      z.string(),
      z
        .object({
          provider: z.string().optional(),
          model: z.string(),
          max_context_size: z.number().int().positive(),
          capabilities: kimiCapabilitiesSchema.optional(),
          overrides: z
            .object({ capabilities: kimiCapabilitiesSchema.optional() })
            .passthrough()
            .optional(),
          display_name: z.string().optional(),
        })
        .passthrough(),
    ),
  })
  .passthrough();

export type KimiProviderConfig = z.infer<typeof kimiConfigSchema>;
export function emptyKimiConfig(): KimiProviderConfig {
  return {
    provider: { type: "openai", base_url: "", api_key: "" },
    models: {},
  };
}

export interface KimiProviderPreset
  extends Omit<PiProviderPreset, "settingsConfig"> {
  settingsConfig: KimiProviderConfig;
}

export const KIMI_ACCOUNT_PRESET_ID = "kimi-official-account";
export const KIMI_OFFICIAL_PROVIDER_NAME = "Kimi Official";

// An action in the account-login group, separate from the API-key catalog.
export const kimiAccountPreset: KimiProviderPreset = {
  name: KIMI_OFFICIAL_PROVIDER_NAME,
  nameKey: "kimi.oauth.title",
  providerKey: KIMI_ACCOUNT_PRESET_ID,
  websiteUrl: "https://www.kimi.com",
  icon: "kimi",
  category: "official",
  settingsConfig: emptyKimiConfig(),
};

type KimiProviderType = KimiProviderConfig["provider"]["type"];
type KimiModel = KimiProviderConfig["models"][string];

// Only native protocol equivalents are admitted. Bedrock's signed Converse API
// has no equivalent in Kimi Code and must not become an OpenAI/Anthropic preset.
const KIMI_API_FORMATS: Partial<Record<PiApiFormat, KimiProviderType>> = {
  "openai-completions": "openai",
  "openai-responses": "openai_responses",
  "anthropic-messages": "anthropic",
  "google-generative-ai": "google-genai",
};

function toKimiModel(model: PiPresetModel, type: KimiProviderType): KimiModel {
  const capabilities: NonNullable<KimiModel["capabilities"]> = ["tool_use"];
  if (model.input.includes("image")) capabilities.push("image_in");
  if (model.reasoning) {
    capabilities.push("thinking");
    if (model.thinkingLevelMap?.off === null)
      capabilities.push("always_thinking");
  }
  if (type === "kimi" && model.compat?.deferredToolsMode === "kimi") {
    capabilities.push("dynamically_loaded_tools");
  }

  const thinking = model.thinkingLevelMap ?? {};
  const efforts = [
    ...new Set(
      Object.entries(thinking)
        .filter(
          ([level, value]) => level !== "off" && typeof value === "string",
        )
        .map(([, value]) => value as string),
    ),
  ];
  const defaultEffort = thinking.medium ?? thinking.high ?? efforts[0];

  return {
    model: model.id,
    display_name: model.name,
    max_context_size: model.contextWindow,
    capabilities,
    ...(type === "anthropic" ? { max_output_size: model.maxTokens } : {}),
    ...(type === "anthropic" && model.compat?.forceAdaptiveThinking === true
      ? { adaptive_thinking: true }
      : {}),
    ...(type === "openai" &&
    model.compat?.requiresReasoningContentOnAssistantMessages === true
      ? { reasoning_key: "reasoning_content" }
      : {}),
    ...(model.reasoning && efforts.length
      ? { support_efforts: efforts, default_effort: defaultEffort }
      : {}),
    ...(model.reasoning && typeof thinking.off === "string"
      ? { off_effort: thinking.off }
      : {}),
  };
}

// Share the reviewed supplier catalog and grouping metadata with Pi; translate
// connection and model fields explicitly, without leaking Pi-only SDK options.
export const kimiProviderPresets: KimiProviderPreset[] =
  piProviderPresets.flatMap((preset) => {
    const { settingsConfig: source, ...identity } = preset;
    let type = KIMI_API_FORMATS[source.api];
    if (!type || source.compat || !source.models.length) return [];
    let baseUrl = source.baseUrl;
    if (preset.family === "kimi") {
      type = "kimi";
      if (source.api === "anthropic-messages")
        baseUrl = `${baseUrl.replace(/\/$/, "")}/v1`;
    }
    return [
      {
        ...identity,
        settingsConfig: {
          provider: {
            type,
            base_url: baseUrl,
            api_key:
              source.apiKey || (preset.category === "local" ? "local" : ""),
            ...(source.headers
              ? { custom_headers: { ...source.headers } }
              : {}),
          },
          models: Object.fromEntries(
            source.models.map((model) => [
              `${preset.providerKey}/${model.id}`,
              toKimiModel(model, type),
            ]),
          ),
        },
      },
    ];
  });
