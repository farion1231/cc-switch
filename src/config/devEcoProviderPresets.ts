import {
  mcodeProviderPresets,
  type McodeProviderPreset,
} from "./mcodeProviderPresets";

export const DEVECO_API_FORMATS = [
  "anthropic-messages",
  "openai-completions",
  "openai-responses",
] as const;

export interface DevEcoProviderPreset
  extends Omit<McodeProviderPreset, "settingsConfig"> {
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

// DevEco Code is an OpenCode fork, so its provider schema is identical to
// Mcode's openai-compatible shape. Reuse the filtered Mcode presets verbatim.
export const devecoProviderPresets: DevEcoProviderPreset[] =
  mcodeProviderPresets.map((preset) => ({
    ...preset,
    settingsConfig: {
      name: preset.settingsConfig.name,
      kind: "custom",
      enabled: true,
      api: preset.settingsConfig.api,
      options: {
        baseURL: preset.settingsConfig.options.baseURL,
        apiKey: preset.settingsConfig.options.apiKey,
        ...(preset.settingsConfig.options.headers
          ? { headers: preset.settingsConfig.options.headers }
          : {}),
      },
      models: preset.settingsConfig.models,
    },
  }));
