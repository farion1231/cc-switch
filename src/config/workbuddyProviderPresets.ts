import { piProviderPresets, type PiProviderPreset } from "./piProviderPresets";
import {
  PI_THINKING_LEVELS,
  clonePiThinkingLevelMap,
  isPiThinkingLevelMap,
  type PiThinkingLevelMap,
} from "./piThinkingProfiles";

/** WorkBuddy 模型选择器里的思考强度（WorkBuddy CLI 的 `low`…`max`，不含 `minimal`）。 */
export const WORKBUDDY_EFFORTS = [
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
] as const;
export type WorkBuddyEffort = (typeof WORKBUDDY_EFFORTS)[number];

/** 模型条目的 `reasoning`：WorkBuddy 按它在模型选择器里列出思考强度。 */
export interface WorkBuddyReasoning {
  supportedEfforts: WorkBuddyEffort[];
  defaultEffort: WorkBuddyEffort;
  canDisableThinking: boolean;
}

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
  reasoning?: WorkBuddyReasoning;
  /** WorkBuddy CLI 按它把所选强度翻译成上游认的 `reasoning_effort`（语义同 Pi） */
  thinkingLevelMap?: PiThinkingLevelMap;
  [key: string]: unknown;
}

/**
 * Pi 的思考映射换成 WorkBuddy 模型条目的思考字段。WorkBuddy CLI 内置的 pi-ai 用同一套语义
 * （`deriveReasoningCapabilities`）：映射里 `null` 的档不支持，`xhigh` / `max` 要显式写出才支持，
 * 其余档默认支持。空映射在 Pi 里表示「用 Pi 运行时的默认」，这里拿不到核对过的档位，不生成，
 * WorkBuddy 只显示思考开关。
 */
export function workbuddyThinkingFields(
  map: unknown,
): Pick<WorkBuddyModel, "reasoning" | "thinkingLevelMap"> {
  if (!isPiThinkingLevelMap(map) || Object.keys(map).length === 0) return {};
  const supported = PI_THINKING_LEVELS.filter((level) => {
    const value = map[level];
    return (
      value !== null &&
      (level === "xhigh" || level === "max" ? value !== undefined : true)
    );
  });
  const supportedEfforts = WORKBUDDY_EFFORTS.filter((effort) =>
    supported.includes(effort),
  );
  if (supportedEfforts.length === 0) return {};
  return {
    reasoning: {
      supportedEfforts,
      defaultEffort: supportedEfforts.includes("high")
        ? "high"
        : supportedEfforts[supportedEfforts.length - 1],
      canDisableThinking: supported.includes("off"),
    },
    thinkingLevelMap: clonePiThinkingLevelMap(map),
  };
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
// 模型不带 `name`：WorkBuddy 菜单把自定义模型显示成 `name:id`，写入时用厂商标签补上名字，
// 显示成「Kimi:kimi-k3.1-pro」，而不是「Kimi K3.1 Pro:kimi-k3.1-pro」把同一个模型说两遍。
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
            ...(model.contextWindow
              ? { maxInputTokens: model.contextWindow }
              : {}),
            ...(model.maxTokens ? { maxOutputTokens: model.maxTokens } : {}),
            supportsToolCall: true,
            supportsImages: model.input?.includes("image") ?? false,
            supportsReasoning: Boolean(model.reasoning),
            ...(model.reasoning
              ? workbuddyThinkingFields(model.thinkingLevelMap)
              : {}),
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
