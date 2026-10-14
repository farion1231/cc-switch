import { piProviderPresets } from "./piProviderPresets";

// Only identical native fields are carried over. Pi's thinkingLevelMap and
// protocol-specific shaping flags have no exact omp equivalent and are omitted.
const COMPAT_KEYS = new Set([
  "supportsStore",
  "supportsDeveloperRole",
  "supportsReasoningEffort",
  "maxTokensField",
  "supportsUsageInStreaming",
  "requiresToolResultName",
  "requiresMistralToolIds",
  "requiresAssistantAfterToolResult",
  "requiresThinkingAsText",
  "supportsStrictMode",
  "openRouterRouting",
  "vercelGatewayRouting",
]);

function ompCompat(compat?: Record<string, unknown>) {
  if (!compat) return {};
  const entries = Object.entries(compat).filter(
    ([key, value]) =>
      COMPAT_KEYS.has(key) ||
      (key === "thinkingFormat" &&
        ["openai", "openrouter", "zai", "qwen", "qwen-chat-template"].includes(
          String(value),
        )),
  );
  return entries.length ? { compat: Object.fromEntries(entries) } : {};
}

// No provider/model exclusions: all Pi preset APIs are supported by omp.
export const ompProviderPresets = piProviderPresets.map((preset) => ({
  ...preset,
  settingsConfig: {
    baseUrl: preset.settingsConfig.baseUrl,
    api: preset.settingsConfig.api,
    apiKey: "",
    ...(preset.settingsConfig.headers
      ? { headers: preset.settingsConfig.headers }
      : {}),
    ...ompCompat(preset.settingsConfig.compat),
    models: preset.settingsConfig.models.map(
      ({ thinkingLevelMap: _thinking, compat, ...model }) => ({
        ...model,
        ...ompCompat(compat),
      }),
    ),
  },
}));

export type OmpProviderPreset = (typeof ompProviderPresets)[number];
