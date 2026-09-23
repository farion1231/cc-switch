import { piProviderPresets, type PiProviderPreset } from "./piProviderPresets";

export const MCODE_API_FORMATS = [
  "anthropic-messages",
  "openai-completions",
  "openai-responses",
] as const;

// Reuse endpoint/model metadata only where the preset needs no Pi-specific compatibility options.
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

export const mcodeProviderPresets: McodeProviderPreset[] = piProviderPresets
  .filter(
    ({ settingsConfig: config }) =>
      MCODE_API_FORMATS.some((api) => api === config.api) &&
      !config.compat &&
      config.models.every((model) => !model.compat),
  )
  .map((preset) => ({
    ...preset,
    settingsConfig: {
      name: preset.name,
      kind: "custom",
      enabled: true,
      api: preset.settingsConfig.api,
      options: {
        baseURL: preset.settingsConfig.baseUrl,
        apiKey: "",
        ...(preset.settingsConfig.headers
          ? { headers: preset.settingsConfig.headers }
          : {}),
      },
      models: Object.fromEntries(
        preset.settingsConfig.models.map((model) => [
          model.id,
          {
            name: model.name,
            reasoning: model.reasoning,
            modalities: { input: model.input, output: ["text"] },
            limit: { context: model.contextWindow, output: model.maxTokens },
          },
        ]),
      ),
    },
  }));
