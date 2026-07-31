import {
  getQoderCliPreset,
  isQoderCliAllowedModel,
  qodercliProviderPresets,
  type QoderCliPlanType,
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

const QODERCLI_PLAN_TYPES: QoderCliPlanType[] = ["pg", "cp", "tp"];

function isQoderCliPlanType(value: string): value is QoderCliPlanType {
  return QODERCLI_PLAN_TYPES.includes(value as QoderCliPlanType);
}

/**
 * Read Qoder's catalog-backed shape. Legacy arbitrary-endpoint fields remain
 * ignored, while model IDs entered through Qoder's "Add model name" flow are
 * preserved when their provider and plan type are supported.
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
        const model = typeof item.model === "string" ? item.model.trim() : "";
        const type = typeof item.type === "string" ? item.type : "";
        const format = typeof item.format === "string" ? item.format : "openai";
        if (!isQoderCliPlanType(type)) {
          return undefined;
        }

        if (
          format !== "openai" ||
          !isQoderCliAllowedModel(provider, {
            model,
            type,
            format,
          })
        ) {
          return undefined;
        }

        const officialModel = preset?.models.find(
          (candidate) =>
            candidate.model === model &&
            candidate.type === type &&
            candidate.format === format,
        );
        if (officialModel) {
          return officialModel;
        }

        const displayName =
          typeof item.displayName === "string" && item.displayName.trim()
            ? item.displayName.trim()
            : model;
        return {
          model,
          type,
          format: "openai" as const,
          displayName,
          ...(typeof item.maxInputTokens === "number"
            ? { maxInputTokens: item.maxInputTokens }
            : {}),
          ...(typeof item.isVl === "boolean" ? { isVl: item.isVl } : {}),
          ...(typeof item.isReasoning === "boolean"
            ? { isReasoning: item.isReasoning }
            : {}),
        } satisfies QoderCliPresetModel;
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
  const selected = models[0];
  const allowedModel =
    selected && isQoderCliAllowedModel(provider, selected)
      ? {
          ...selected,
          model: selected.model.trim(),
          displayName: selected.displayName.trim() || selected.model.trim(),
          format: "openai" as const,
        }
      : undefined;

  const config: QoderCliProviderConfig = {
    provider: provider.trim(),
    apiKey: apiKey.trim(),
    models: allowedModel ? [allowedModel] : [],
  };
  return JSON.stringify(config, null, 2);
}
