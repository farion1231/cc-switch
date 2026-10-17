import {
  piProviderPresets,
  type PiPresetModel,
  type PiProviderPreset,
} from "./piProviderPresets";

/**
 * API kinds `~/.commandcode/providers.json` accepts; `openai-completions` is
 * the default and is omitted from the file.
 */
export const COMMANDCODE_API_FORMATS = [
  "openai-completions",
  "openai-responses",
  "anthropic-messages",
] as const;

export type CommandCodeApiFormat = (typeof COMMANDCODE_API_FORMATS)[number];

/**
 * A `providers.json` model entry. Command Code also reads the
 * `limit.context` / `limit.output` aliases, but writes `contextWindow` /
 * `maxOutput`, so presets keep the written form.
 */
export interface CommandCodeProviderModel {
  name?: string;
  contextWindow?: number;
  maxOutput?: number;
  reasoning?: boolean;
  [key: string]: unknown;
}

/** A `providers.json` entry (`apiKey: false` marks a keyless endpoint). */
export interface CommandCodeProviderConfig {
  name?: string;
  api?: CommandCodeApiFormat;
  baseURL: string;
  apiKey?: string | false;
  headers?: Record<string, string>;
  models: Record<string, CommandCodeProviderModel>;
}

// Reuse Pi's curated endpoint/model metadata: Command Code speaks the same wire
// formats, so only presets in a supported API kind carry over, and only while
// they do not depend on Pi-specific request shaping (`compat`).
export interface CommandCodeProviderPreset
  extends Omit<PiProviderPreset, "settingsConfig"> {
  settingsConfig: CommandCodeProviderConfig;
}

const toCommandCodeModel = (
  model: PiPresetModel,
): CommandCodeProviderModel => ({
  name: model.name,
  ...(model.contextWindow ? { contextWindow: model.contextWindow } : {}),
  ...(model.maxTokens ? { maxOutput: model.maxTokens } : {}),
  ...(typeof model.reasoning === "boolean"
    ? { reasoning: model.reasoning }
    : {}),
});

export const commandcodeProviderPresets: CommandCodeProviderPreset[] =
  piProviderPresets.flatMap((preset) => {
    const config = preset.settingsConfig;
    const api = COMMANDCODE_API_FORMATS.find((format) => format === config.api);
    if (!api || config.compat || config.models.length === 0) return [];
    return [
      {
        ...preset,
        settingsConfig: {
          api,
          baseURL: config.baseUrl,
          ...(config.headers ? { headers: config.headers } : {}),
          models: Object.fromEntries(
            config.models.map((model) => [model.id, toCommandCodeModel(model)]),
          ),
        },
      },
    ];
  });
