import { piProviderPresets, type PiProviderPreset } from "./piProviderPresets";

/**
 * WorkBuddy 自定义模型（`~/.workbuddy/models.json`）里单个模型条目的字段，
 * 与 WorkBuddy / CodeBuddy Code 文档的 models.json 字段一致。
 */
export interface WorkBuddyModel {
  id: string;
  name?: string;
  maxInputTokens?: number;
  maxOutputTokens?: number;
  supportsToolCall?: boolean;
  supportsImages?: boolean;
  supportsReasoning?: boolean;
  [key: string]: unknown;
}

/**
 * CC Switch 侧的供应商配置：`vendor` / `url` / `apiKey` 写入时复制到每个模型条目。
 * 只走 OpenAI Chat Completions，`url` 填完整的 `/chat/completions` 地址。
 * 旧条目可能带 `api` 字段（以前可选 Responses），编辑保存时原样保留、同样复制到条目。
 */
export interface WorkBuddySettingsConfig {
  vendor?: string;
  url: string;
  apiKey: string;
  api?: string;
  models: WorkBuddyModel[];
  [key: string]: unknown;
}

export interface WorkBuddyProviderPreset
  extends Omit<PiProviderPreset, "settingsConfig"> {
  settingsConfig: WorkBuddySettingsConfig;
}

export const WORKBUDDY_DEFAULT_CONFIG: WorkBuddySettingsConfig = {
  vendor: "",
  url: "",
  apiKey: "",
  models: [],
};

/** WorkBuddy 自定义模型要求填完整接口地址。 */
export function chatCompletionsUrl(baseUrl: string): string {
  const trimmed = baseUrl.trim().replace(/\/+$/, "");
  return /\/chat\/completions$/.test(trimmed)
    ? trimmed
    : `${trimmed}/chat/completions`;
}

// 复用 Pi 预设的端点与模型元数据。只取 OpenAI Chat Completions 协议的预设，其余协议跳过；
// 依赖 Pi 供应商级兼容开关的预设 WorkBuddy 表达不了，也跳过。
export const workbuddyProviderPresets: WorkBuddyProviderPreset[] =
  piProviderPresets.flatMap((preset) => {
    const config = preset.settingsConfig;
    if (config.compat || config.headers) return [];
    if (config.api !== "openai-completions") return [];
    if (config.models.length === 0) return [];
    return [
      {
        ...preset,
        settingsConfig: {
          vendor: preset.name,
          url: chatCompletionsUrl(config.baseUrl),
          apiKey: "",
          models: config.models.map((model) => ({
            id: model.id,
            ...(model.name ? { name: model.name } : {}),
            ...(model.contextWindow
              ? { maxInputTokens: model.contextWindow }
              : {}),
            ...(model.maxTokens ? { maxOutputTokens: model.maxTokens } : {}),
            supportsToolCall: true,
            supportsImages: model.input?.includes("image") ?? false,
            supportsReasoning: Boolean(model.reasoning),
          })),
        },
      },
    ];
  });

/** 授权中心里 WorkBuddy 账号的 provider 标识（官方登录卡的 `meta.authBinding.authProvider`）。 */
export const WORKBUDDY_ACCOUNT_AUTH_PROVIDER = "workbuddy_account";

/**
 * 「WorkBuddy 官方登录」：不是自定义模型，卡片只绑定授权中心里的一个 WorkBuddy 账号，
 * 「切换」时把 WorkBuddy 桌面端的登录换成这个号。不写进 models.json。
 */
export const workbuddyOfficialPreset: WorkBuddyProviderPreset = {
  name: "WorkBuddy 官方登录",
  nameKey: "workbuddy.official.presetName",
  providerKey: "workbuddy-official",
  websiteUrl: "",
  category: "official",
  icon: "workbuddy",
  settingsConfig: WORKBUDDY_DEFAULT_CONFIG,
};
