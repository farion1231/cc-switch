import {
  piProviderPresets,
  type PiPresetModel,
  type PiProviderPreset,
} from "./piProviderPresets";
import type { PiThinkingLevel } from "./piThinkingProfiles";

export const MCODE_API_FORMATS = [
  "anthropic-messages",
  "openai-completions",
  "openai-responses",
] as const;

// Reuse Pi's endpoint/model metadata; models whose compat MCode cannot express are skipped.
export interface McodeProviderPreset
  extends Omit<PiProviderPreset, "settingsConfig"> {
  settingsConfig: {
    name: string;
    kind: string;
    enabled: boolean;
    api: string;
    options: {
      baseURL: string;
      apiKey: string;
      headers?: Record<string, string>;
    };
    models: Record<string, import("@/types").OpenCodeModel>;
  };
}

// The `models.<id>.compat` keys MCode (0.5.x) reads from config.yaml and layers
// over the compat it synthesizes itself. Older MCode ignores the block and falls
// back to plain requests.
const MCODE_COMPAT_FLAGS = new Set([
  "supportsStore",
  "supportsDeveloperRole",
  "supportsReasoningEffort",
  "supportsUsageInStreaming",
  "requiresToolResultName",
  "requiresAssistantAfterToolResult",
  "requiresThinkingAsText",
  "requiresReasoningContentOnAssistantMessages",
  "supportsStrictMode",
  "supportsLongCacheRetention",
  "supportsEagerToolInputStreaming",
  "supportsCacheControlOnTools",
  "supportsTemperature",
  "sendSessionAffinityHeaders",
  "sendSessionIdHeader",
  "zaiToolStream",
  "forceAdaptiveThinking",
  "allowEmptySignature",
]);

const MCODE_COMPAT_ENUMS: Record<string, readonly string[]> = {
  maxTokensField: ["max_tokens", "max_completion_tokens"],
  thinkingFormat: [
    "openai",
    "openrouter",
    "together",
    "deepseek",
    "zai",
    "qwen",
    "qwen-chat-template",
    "string-thinking",
    "ant-ling",
  ],
  cacheControlFormat: ["anthropic"],
};

const isMcodeCompatEntry = ([key, value]: [string, unknown]) =>
  MCODE_COMPAT_FLAGS.has(key)
    ? typeof value === "boolean"
    : typeof value === "string" &&
      (MCODE_COMPAT_ENUMS[key]?.includes(value) ?? false);

// MCode would silently drop any other key, so a model relying on one is skipped.
const isMcodeCompatible = (model: PiPresetModel) =>
  Object.entries(model.compat ?? {}).every(isMcodeCompatEntry);

// MCode does not read Pi's thinkingLevelMap. Adaptive-thinking models get it as
// `thinking.effortOptions` instead: with those, turning thinking on always picks
// an effort, which keeps even compat-blind MCode versions off budget_tokens.
// Values are Pi's Anthropic adaptive effort for levels the map leaves unset.
const ADAPTIVE_EFFORT_FALLBACKS: [PiThinkingLevel, string][] = [
  ["minimal", "low"],
  ["low", "low"],
  ["medium", "medium"],
  ["high", "high"],
  ["xhigh", "high"],
  ["max", "high"],
];

function adaptiveEffortOptions(model: PiPresetModel): string[] {
  const efforts = ADAPTIVE_EFFORT_FALLBACKS.flatMap(([level, fallback]) => {
    const mapped = model.thinkingLevelMap?.[level];
    return mapped === null ? [] : [mapped ?? fallback];
  });
  return [...new Set(efforts)];
}

export const mcodeProviderPresets: McodeProviderPreset[] =
  piProviderPresets.flatMap((preset) => {
    const config = preset.settingsConfig;
    const api = MCODE_API_FORMATS.find((format) => format === config.api);
    if (!api || config.compat) return [];
    const models = config.models.filter(isMcodeCompatible);
    if (models.length === 0) return [];
    return [
      {
        ...preset,
        settingsConfig: {
          name: preset.name,
          kind: "custom",
          enabled: true,
          api,
          options: {
            baseURL: config.baseUrl,
            apiKey: "",
            ...(config.headers ? { headers: config.headers } : {}),
          },
          models: Object.fromEntries(
            models.map((model) => [
              model.id,
              {
                name: model.name,
                reasoning: model.reasoning,
                modalities: { input: model.input, output: ["text"] },
                limit: {
                  context: model.contextWindow,
                  output: model.maxTokens,
                },
                ...(model.compat ? { compat: model.compat } : {}),
                ...(model.compat?.forceAdaptiveThinking === true
                  ? {
                      thinking: {
                        effortOptions: adaptiveEffortOptions(model),
                      },
                    }
                  : {}),
              },
            ]),
          ),
        },
      },
    ];
  });
