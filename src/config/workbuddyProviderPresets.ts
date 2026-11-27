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
 * 与 WorkBuddy / CodeBuddy 文档的 models.json 字段一致。
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
 * 编辑用的映射：有映射就用它；只手写了 `reasoning` 的，按列出的强度还原；
 * 都没有时所有档位从「不支持」起步（空映射会被当成 Pi 默认，档位全开）。
 */
function editableThinkingMap(model: WorkBuddyModel): PiThinkingLevelMap {
  if (
    isPiThinkingLevelMap(model.thinkingLevelMap) &&
    Object.keys(model.thinkingLevelMap).length > 0
  ) {
    return clonePiThinkingLevelMap(model.thinkingLevelMap);
  }
  const map = listedEffortsMap(model);
  if (model.reasoning && !model.reasoning.canDisableThinking) map.off = null;
  return map;
}

/** 按 `reasoning` 列出的强度写成显式映射：列出的发同名值，其余不支持。 */
function listedEffortsMap(model: WorkBuddyModel): PiThinkingLevelMap {
  const map: PiThinkingLevelMap = { minimal: null };
  const listed = Array.isArray(model.reasoning?.supportedEfforts)
    ? model.reasoning.supportedEfforts
    : [];
  for (const effort of WORKBUDDY_EFFORTS) {
    map[effort] = listed.includes(effort) ? effort : null;
  }
  return map;
}

/** 按新映射重写模型的思考字段；一个强度都不剩时去掉，WorkBuddy 只显示思考开关。 */
function withThinkingMap(
  model: WorkBuddyModel,
  map: PiThinkingLevelMap,
): WorkBuddyModel {
  const { reasoning: _reasoning, thinkingLevelMap: _map, ...rest } = model;
  const fields = workbuddyThinkingFields(map);
  if (!fields.reasoning) return rest;
  const previous = model.reasoning?.defaultEffort;
  if (previous && fields.reasoning.supportedEfforts.includes(previous)) {
    fields.reasoning.defaultEffort = previous;
  }
  return { ...rest, ...fields };
}

/** 表单里勾选 / 取消一个思考强度。已有的上游值（预设核对过的映射）勾回来时保留。 */
export function setWorkBuddyEffort(
  model: WorkBuddyModel,
  effort: WorkBuddyEffort,
  enabled: boolean,
): WorkBuddyModel {
  const map = editableThinkingMap(model);
  if (!enabled) map[effort] = null;
  else if (typeof map[effort] !== "string") map[effort] = effort;
  return withThinkingMap(model, map);
}

/** 表单里切换「可以关闭思考」。 */
export function setWorkBuddyCanDisableThinking(
  model: WorkBuddyModel,
  enabled: boolean,
): WorkBuddyModel {
  const map = editableThinkingMap(model);
  if (!enabled) map.off = null;
  else if (map.off === null) {
    delete map.off;
    // 只写了 `{ off: null }` 的映射（Kimi K2.7 Code 等）删掉 off 就空了，空映射会被
    // 当成「不生成强度」；把正在列出的强度写成显式映射，保住原有档位。
    if (Object.keys(map).length === 0)
      Object.assign(map, listedEffortsMap(model));
  }
  return withThinkingMap(model, map);
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

export { WORKBUDDY_ACCOUNT_AUTH_PROVIDER } from "./workbuddyOfficialMeta";

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
