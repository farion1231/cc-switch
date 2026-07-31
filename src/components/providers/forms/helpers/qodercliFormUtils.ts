import {
  getQoderCliPreset,
  qodercliProviderPresets,
  type QoderCliPresetModel,
  type QoderCliProviderConfig,
} from "@/config/qodercliProviderPresets";

const defaultPreset = qodercliProviderPresets[0];

export const QODERCLI_DEFAULT_CONFIG = JSON.stringify(
  defaultPreset.settingsConfig,
  null,
  2,
);

export interface ParsedQoderCliConfig {
  provider: string;
  apiKey: string;
  models: QoderCliPresetModel[];
}

/**
 * Read the current official shape. Legacy arbitrary-endpoint fields are
 * intentionally ignored; they are not accepted by Qoder's BYOK service.
 */
export function parseQoderCliConfig(
  value?: string | Record<string, unknown>,
): ParsedQoderCliConfig {
  try {
    const raw =
      typeof value === "string"
        ? JSON.parse(value || QODERCLI_DEFAULT_CONFIG)
        : (value ?? JSON.parse(QODERCLI_DEFAULT_CONFIG));
    const parsed = raw as Record<string, unknown>;
    const provider =
      typeof parsed.provider === "string" ? parsed.provider.trim() : "";
    const preset = getQoderCliPreset(provider);
    const rawModels = Array.isArray(parsed.models) ? parsed.models : [];
    const models = rawModels
      .filter(
        (item): item is Record<string, unknown> =>
          !!item && typeof item === "object",
      )
      .map((item) => {
        const model = typeof item.model === "string" ? item.model : "";
        const type = typeof item.type === "string" ? item.type : "";
        const format = typeof item.format === "string" ? item.format : "";
        return preset?.models.find(
          (candidate) =>
            candidate.model === model &&
            candidate.type === type &&
            candidate.format === format,
        );
      })
      .filter((item): item is QoderCliPresetModel => item !== undefined);

    return {
      provider,
      apiKey: typeof parsed.apiKey === "string" ? parsed.apiKey : "",
      models,
    };
  } catch {
    return { provider: "", apiKey: "", models: [] };
  }
}

export function buildQoderCliConfigJson(
  provider: string,
  apiKey: string,
  models: QoderCliPresetModel[],
): string {
  const preset = getQoderCliPreset(provider);
  const selected = models[0];
  const officialModel = selected
    ? preset?.models.find(
        (candidate) =>
          candidate.model === selected.model &&
          candidate.type === selected.type &&
          candidate.format === selected.format,
      )
    : undefined;

  const config: QoderCliProviderConfig = {
    provider: provider.trim(),
    apiKey: apiKey.trim(),
    models: officialModel ? [officialModel] : [],
  };
  return JSON.stringify(config, null, 2);
}
