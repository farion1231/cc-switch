import type { QoderAppId, QoderModel, QoderProvider } from "@/lib/api/qoder";
import {
  piProviderPresets,
  type PiApiFormat,
  type PiPresetModel,
  type PiProviderPreset,
} from "./piProviderPresets";

export interface QoderProviderPreset
  extends Omit<PiProviderPreset, "settingsConfig"> {
  settingsConfig: QoderProvider;
}

export const QODER_ACCOUNT_PRESET_ID = "qoder-official-account";

export function qoderAccountPreset(app: QoderAppId): QoderProviderPreset {
  return {
    name: app === "qoder-cn" ? "Qoder CN" : "Qoder",
    nameKey:
      app === "qoder-cn" ? "qoder.officialAccountCn" : "qoder.officialAccount",
    providerKey: QODER_ACCOUNT_PRESET_ID,
    websiteUrl:
      app === "qoder-cn" ? "https://qoder.com.cn" : "https://qoder.com",
    icon: "qoder",
    category: "official",
    // This picker entry opens native account login, never an API-key form.
    settingsConfig: { baseUrl: "", apiKey: "" },
  };
}

type Protocol = NonNullable<QoderProvider["protocol"]>;
const PROTOCOLS: Partial<Record<PiApiFormat, Protocol>> = {
  "openai-completions": "openai",
  "openai-responses": "openai-responses",
  "anthropic-messages": "anthropic",
};
const EFFORTS = new Set(["low", "medium", "high", "xhigh", "max"]);

function toQoderModel(model: PiPresetModel, protocol: Protocol): QoderModel {
  const adaptive =
    protocol === "anthropic" && model.compat?.forceAdaptiveThinking === true;
  const efforts = [
    ...new Set(
      Object.values(model.thinkingLevelMap ?? {}).filter(
        (value): value is string =>
          typeof value === "string" && EFFORTS.has(value),
      ),
    ),
  ];
  const maxTokensField = model.compat?.maxTokensField;
  return {
    model: model.id,
    displayName: model.name,
    contextWindow: model.contextWindow,
    maxOutputTokens: model.maxTokens,
    capabilities: {
      tools: true,
      vision: model.input.includes("image"),
      thinking: model.reasoning
        ? {
            modes: [adaptive ? "adaptive" : "enabled"],
            ...(!adaptive
              ? { requiresBudgetForEnabled: protocol === "anthropic" }
              : {}),
            ...(efforts.length ? { supportedEffortLevels: efforts } : {}),
          }
        : { modes: [] },
    },
    ...(maxTokensField === "max_tokens" ||
    maxTokensField === "max_completion_tokens"
      ? { maxTokensField }
      : {}),
  };
}

// Reuse the reviewed supplier identities, groups and model catalog. Translate
// only native Qoder fields; Google/Bedrock and custom request headers have no
// equivalent in Qoder's provider loader.
export const qoderProviderPresets: QoderProviderPreset[] =
  piProviderPresets.flatMap(({ settingsConfig: source, ...identity }) => {
    const protocol = PROTOCOLS[source.api];
    if (!protocol || source.compat || source.headers || !source.models.length)
      return [];
    const models = source.models.map((model) => toQoderModel(model, protocol));
    return [
      {
        ...identity,
        settingsConfig: {
          displayName: identity.name,
          type: "openai-compatible",
          protocol,
          authType: protocol === "anthropic" ? "api-key" : "bearer",
          baseUrl: source.baseUrl,
          apiKey: source.apiKey,
          model: models[0].model,
          models,
        },
      },
    ];
  });
