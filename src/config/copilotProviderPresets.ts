import { piProviderPresets, type PiProviderPreset } from "./piProviderPresets";
import {
  type CopilotByokConfig,
  type CopilotProviderNode,
  normalizeCopilotProviderId,
  copilotCatalogOutputLimit,
} from "./copilotConfig";

export * from "./copilotConfig";

export interface CopilotOfficialSettingsConfig {
  env: Record<string, string>;
  reasoningEffort?: string;
}

export interface CopilotProviderPreset
  extends Omit<PiProviderPreset, "settingsConfig"> {
  /** 官方卡是 `{ env }`；自带模型卡是 `{ provider, models }`，`providerKey` 是默认卡片 id */
  settingsConfig: CopilotOfficialSettingsConfig | CopilotByokConfig;
  /** 官方卡模型输入框的下拉候选 */
  models?: string[];
}

export type CopilotByokPreset = Omit<
  CopilotProviderPreset,
  "settingsConfig"
> & { settingsConfig: CopilotByokConfig };

export const COPILOT_OFFICIAL_PRESET_ID = "copilot-official";

/** 用 Copilot CLI 自己的 GitHub 登录；模型可留空（用 Copilot 的默认 / `/model` 选择）。 */
export const copilotOfficialPreset: CopilotProviderPreset = {
  name: "GitHub Copilot",
  providerKey: COPILOT_OFFICIAL_PRESET_ID,
  websiteUrl: "https://github.com/features/copilot/cli",
  icon: "githubcopilot",
  iconColor: "currentColor",
  category: "official",
  settingsConfig: { env: {} },
  models: [
    "auto",
    "claude-sonnet-5.5",
    "claude-opus-5.5",
    "claude-haiku-5.5",
    "gpt-6-sol",
    "gpt-6-luna",
    "gpt-5.6-terra",
  ],
};

/** Pi 的接口格式 → Copilot CLI 的协议与接口。Gemini、Bedrock 等没有对应协议，不提供预设。 */
function copilotProtocol(
  api: PiProviderPreset["settingsConfig"]["api"],
): Pick<CopilotProviderNode, "type" | "wireApi"> | null {
  switch (api) {
    case "openai-completions":
      return { type: "openai", wireApi: "completions" };
    case "openai-responses":
      return { type: "openai", wireApi: "responses" };
    case "anthropic-messages":
      return { type: "anthropic" };
    default:
      return null;
  }
}

/** 预设的默认卡片 id：Pi 预设键去掉 `cc-switch-` 前缀（`/model` 里显示成 `deep-seek/<模型>`）。 */
export function copilotPresetProviderId(providerKey: string): string {
  return normalizeCopilotProviderId(providerKey.replace(/^cc-switch-/, ""));
}

// 复用 Pi 已维护的供应商目录（分组、合作信息、地址、模型），换算成 providers.json 的条目。
export const copilotProviderPresets: CopilotByokPreset[] =
  piProviderPresets.flatMap((preset) => {
    const { settingsConfig: source, ...identity } = preset;
    const protocol = copilotProtocol(source.api);
    if (!protocol || !source.models.length || !source.baseUrl) return [];
    const provider: CopilotProviderNode = {
      ...protocol,
      baseUrl: source.baseUrl,
      apiKey: source.apiKey || (preset.category === "local" ? "local" : ""),
    };
    if (source.headers && Object.keys(source.headers).length) {
      provider.headers = { ...source.headers };
    }
    return [
      {
        ...identity,
        providerKey: copilotPresetProviderId(preset.providerKey),
        settingsConfig: {
          provider,
          models: source.models.map((model) => ({
            id: model.id,
            ...(model.name?.trim() && model.name.trim() !== model.id
              ? { name: model.name.trim() }
              : {}),
            ...(model.contextWindow > 0
              ? { maxContextWindowTokens: model.contextWindow }
              : {}),
            maxOutputTokens: copilotCatalogOutputLimit(
              model.maxTokens,
              model.contextWindow,
            ),
          })),
        },
      },
    ];
  });
