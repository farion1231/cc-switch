import type { ProviderCategory } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";
import {
  hermesProviderPresets,
  type HermesApiMode,
  type HermesProviderPreset,
} from "./hermesProviderPresets";

export type KimiProviderType =
  | "kimi"
  | "anthropic"
  | "openai"
  | "openai_responses"
  | "google-genai"
  | "vertexai";

export type KimiCapability =
  | "thinking"
  | "image_in"
  | "video_in"
  | "audio_in"
  | "tool_use";

export interface KimiProviderConfig {
  type: KimiProviderType;
  api_key?: string;
  base_url?: string;
  oauth?: boolean;
  env?: Record<string, string>;
  custom_headers?: Record<string, string>;
}

export interface KimiModelConfig {
  provider: string;
  model: string;
  max_context_size?: number;
  max_output_size?: number;
  capabilities?: KimiCapability[];
  display_name?: string;
  reasoning_key?: string;
  adaptive_thinking?: boolean;
}

export interface KimiModelEntry extends KimiModelConfig {
  id: string;
}

export interface KimiEditorConfig {
  default_model?: string;
  default_thinking?: boolean;
  default_permission_mode?: string;
  default_plan_mode?: boolean;
  merge_all_available_skills?: boolean;
  telemetry?: boolean;
  providers?: Record<string, KimiProviderConfig>;
  models?: Record<string, KimiModelConfig>;
  thinking?: {
    mode?: string;
    effort?: string;
  };
  loop_control?: {
    max_retries_per_step?: number;
    reserved_context_size?: number;
  };
  background?: {
    max_running_tasks?: number;
    keep_alive_on_exit?: boolean;
  };
  experimental?: {
    micro_compaction?: boolean;
  };
}

export interface KimiProviderSettingsConfig {
  config: KimiEditorConfig;
}

export interface KimiProviderPreset {
  name: string;
  nameKey?: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: KimiProviderSettingsConfig;
  providerKey: string;
  isOfficial?: boolean;
  isPartner?: boolean;
  primePartner?: boolean;
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  templateValues?: Record<string, TemplateValueConfig>;
  theme?: PresetTheme;
  icon?: string;
  iconColor?: string;
  isCustomTemplate?: boolean;
}

export const kimiProviderTypes: Array<{
  value: KimiProviderType;
  label: string;
}> = [
  { value: "kimi", label: "Kimi" },
  { value: "anthropic", label: "Anthropic Messages" },
  { value: "openai", label: "OpenAI Chat Completions" },
  { value: "openai_responses", label: "OpenAI Responses" },
  { value: "google-genai", label: "Google GenAI" },
  { value: "vertexai", label: "Vertex AI" },
];

export const kimiCapabilities: KimiCapability[] = [
  "thinking",
  "tool_use",
  "image_in",
  "video_in",
  "audio_in",
];

export const kimiOfficialProviderKey = "managed:kimi-code";
export const kimiOfficialModelKey = "kimi-code/kimi-for-coding";

export const kimiDefaultSettingsConfig: KimiProviderSettingsConfig = {
  config: {
    default_model: kimiOfficialModelKey,
    default_thinking: true,
    default_permission_mode: "manual",
    default_plan_mode: false,
    merge_all_available_skills: true,
    telemetry: true,
    providers: {
      [kimiOfficialProviderKey]: {
        type: "kimi",
        api_key: "",
        base_url: "https://api.kimi.com/coding/v1",
        env: {
          KIMI_API_KEY: "",
          KIMI_BASE_URL: "https://api.kimi.com/coding/v1",
        },
        custom_headers: {},
      },
    },
    models: {
      [kimiOfficialModelKey]: {
        provider: kimiOfficialProviderKey,
        model: "kimi-for-coding",
        max_context_size: 262144,
        max_output_size: 32000,
        capabilities: ["thinking", "tool_use"],
        display_name: "Kimi For Coding",
      },
    },
    thinking: {
      mode: "auto",
      effort: "high",
    },
    loop_control: {
      max_retries_per_step: 3,
      reserved_context_size: 50000,
    },
    background: {
      max_running_tasks: 4,
      keep_alive_on_exit: false,
    },
    experimental: {
      micro_compaction: true,
    },
  },
};

function mapHermesApiMode(mode?: HermesApiMode): KimiProviderType | null {
  switch (mode) {
    case "anthropic_messages":
      return "anthropic";
    case "codex_responses":
      return "openai_responses";
    case "chat_completions":
    case undefined:
      return "openai";
    case "bedrock_converse":
      return null;
    default:
      return "openai";
  }
}

function mapHermesPresetToKimi(
  preset: HermesProviderPreset,
): KimiProviderPreset | null {
  const providerKey = preset.settingsConfig.name?.trim();
  const providerType = mapHermesApiMode(preset.settingsConfig.api_mode);
  if (!providerKey || !providerType) return null;

  const models = preset.settingsConfig.models ?? [];
  const modelEntries = Object.fromEntries(
    models.map((model) => {
      const modelKey = `${providerKey}/${model.id}`;
      return [
        modelKey,
        {
          provider: providerKey,
          model: model.id,
          ...(model.context_length
            ? { max_context_size: model.context_length }
            : {}),
          ...(model.name ? { display_name: model.name } : {}),
          capabilities: ["thinking", "tool_use"] satisfies KimiCapability[],
        },
      ];
    }),
  );
  const defaultModel = models[0] ? `${providerKey}/${models[0].id}` : undefined;

  return {
    name: preset.nameKey ? preset.name : preset.name,
    nameKey: preset.nameKey,
    websiteUrl: preset.websiteUrl,
    apiKeyUrl: preset.apiKeyUrl,
    providerKey,
    settingsConfig: {
      config: {
        ...(defaultModel ? { default_model: defaultModel } : {}),
        providers: {
          [providerKey]: {
            type: providerType,
            api_key: preset.settingsConfig.api_key ?? "",
            base_url: preset.settingsConfig.base_url ?? "",
            env: {},
            custom_headers: {},
          },
        },
        models: modelEntries,
        thinking: {
          mode: "auto",
          effort: "medium",
        },
      },
    },
    category: preset.category,
    isOfficial: preset.isOfficial,
    isPartner: preset.isPartner,
    primePartner: preset.primePartner,
    partnerPromotionKey: preset.partnerPromotionKey,
    templateValues: preset.templateValues,
    theme: preset.theme,
    icon: preset.icon,
    iconColor: preset.iconColor,
    isCustomTemplate: preset.isCustomTemplate,
  };
}

const importedHermesPresets = hermesProviderPresets
  .filter((preset) => preset.name !== "Kimi For Coding")
  .map(mapHermesPresetToKimi)
  .filter((preset): preset is KimiProviderPreset => preset !== null);

export const kimiProviderPresets: KimiProviderPreset[] = [
  {
    name: "Kimi Code",
    websiteUrl: "https://www.kimi.com/code/docs/?aff=cc-switch",
    apiKeyUrl: "https://platform.moonshot.cn/console/api-keys",
    providerKey: kimiOfficialProviderKey,
    settingsConfig: kimiDefaultSettingsConfig,
    isOfficial: true,
    primePartner: true,
    category: "official",
    icon: "kimi",
    iconColor: "#6366F1",
  },
  ...importedHermesPresets,
];
