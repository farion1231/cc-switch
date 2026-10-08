// 从拉取列表选中模型后，把查到的已知参数换成各应用的字段格式补进这一行。
// 统一规则：只补空着的字段，用户填过的值一律不动；布尔和模态只往「支持」方向补。

import type { KnownModelMetadata } from "@/lib/modelMetadata";
import type { CodexCatalogModel, OpenClawModel, OpenCodeModel } from "@/types";
import type { HermesModel } from "@/config/hermesProviderPresets";
import {
  PI_THINKING_LEVELS,
  type PiThinkingLevelMap,
} from "@/config/piThinkingProfiles";

/** 补全前后是否不同：没补上任何字段时不提示用户。 */
export const metadataFilledAnything = (before: unknown, after: unknown) =>
  JSON.stringify(before) !== JSON.stringify(after);

const isBlank = (value: unknown) =>
  value === undefined || value === null || String(value).trim() === "";

/** 只认文字和图片两种输入（Codex / OpenClaw / Pi 的模态字段只有这两种）。 */
function textImageModalities(
  modalities: string[] | undefined,
): string[] | undefined {
  if (!modalities) return undefined;
  return modalities.includes("image") ? ["text", "image"] : ["text"];
}

export function fillCodexCatalogModel<T extends CodexCatalogModel>(
  row: T,
  metadata: KnownModelMetadata,
  knownLevels: readonly string[],
): T {
  const next = { ...row };
  if (isBlank(row.contextWindow) && metadata.contextWindow) {
    next.contextWindow = String(metadata.contextWindow);
  }
  if (!row.reasoningLevels?.length && metadata.reasoningEfforts) {
    // 按 Codex 的档位顺序排，丢掉它不认识的值。
    const levels = knownLevels.filter((level) =>
      metadata.reasoningEfforts?.includes(level),
    );
    if (levels.length > 0) next.reasoningLevels = levels;
  }
  if (!row.inputModalities && metadata.inputModalities) {
    next.inputModalities = textImageModalities(metadata.inputModalities);
  }
  return next;
}

export function fillOpenClawModel(
  model: OpenClawModel,
  metadata: KnownModelMetadata,
): OpenClawModel {
  const next = { ...model };
  if (isBlank(model.contextWindow) && metadata.contextWindow) {
    next.contextWindow = metadata.contextWindow;
  }
  if (isBlank(model.maxTokens) && metadata.maxOutputTokens) {
    next.maxTokens = metadata.maxOutputTokens;
  }
  if (model.reasoning === undefined && metadata.reasoning !== undefined) {
    next.reasoning = metadata.reasoning;
  } else if (metadata.reasoning === true) {
    next.reasoning = true;
  }
  if (!model.input?.length) {
    const input = textImageModalities(metadata.inputModalities);
    if (input) next.input = input;
  } else if (
    metadata.inputModalities?.includes("image") &&
    !model.input.includes("image")
  ) {
    // 新建的行默认只有 text，模型支持图片就补上。
    next.input = [...model.input, "image"];
  }
  if (!model.cost && metadata.cost) {
    next.cost = { ...metadata.cost };
  }
  return next;
}

export function fillHermesModel(
  model: HermesModel,
  metadata: KnownModelMetadata,
): HermesModel {
  return isBlank(model.context_length) && metadata.contextWindow
    ? { ...model, context_length: metadata.contextWindow }
    : model;
}

export function fillOpenCodeModel(
  model: OpenCodeModel,
  metadata: KnownModelMetadata,
): OpenCodeModel {
  const next = { ...model };
  const limit = { ...(model.limit ?? {}) };
  if (isBlank(limit.context) && metadata.contextWindow) {
    limit.context = metadata.contextWindow;
  }
  if (isBlank(limit.output) && metadata.maxOutputTokens) {
    limit.output = metadata.maxOutputTokens;
  }
  if (Object.keys(limit).length > 0) next.limit = limit;
  if (model.modalities === undefined && metadata.inputModalities) {
    next.modalities = {
      input: metadata.inputModalities,
      output: metadata.outputModalities ?? ["text"],
    };
  }
  // OpenCode 只给 reasoning 为真的模型生成思考档位（ctrl+t）。只补 true：
  // 写 false 会盖住 models.dev 同名条目，用户明确写的 false 也不动。
  if (model.reasoning === undefined && metadata.reasoning === true) {
    next.reasoning = true;
  }
  return next;
}

const PI_EFFORT_LEVELS = PI_THINKING_LEVELS.filter((level) => level !== "off");

/**
 * 把 effort 档位转成 Pi 的 thinkingLevelMap，规则照搬 Pi 官方从 models.dev 生成
 * 内置模型目录的 `getEffortThinkingLevelMap`：列出的档位发同名值，没列出的写
 * `null` 隐藏；`off` 在列出 `none` 时发 `"none"`，否则同样隐藏。一个 Pi 档位都
 * 对不上时不生成。
 */
export function piThinkingLevelMapFromEfforts(
  efforts: readonly string[] | undefined,
): PiThinkingLevelMap | undefined {
  const supported = new Set(efforts);
  if (
    !PI_EFFORT_LEVELS.some((level) => supported.has(level)) &&
    !supported.has("none")
  ) {
    return undefined;
  }
  const map: PiThinkingLevelMap = {
    off: supported.has("none") ? "none" : null,
  };
  for (const level of PI_EFFORT_LEVELS) {
    map[level] = supported.has(level) ? level : null;
  }
  return map;
}

export interface PiProviderProtocol {
  api: string;
  baseUrl: string;
  providerId: string;
  compat: Record<string, unknown>;
}

/**
 * Pi 运行时对 Chat Completions 的思考行为探测（`detectCompat`）：按供应商 ID 和
 * 地址认出思考格式、是否发送 `reasoning_effort`，compat 里写明的值优先。
 */
function piCompletionsThinking({
  baseUrl,
  providerId,
  compat,
}: Omit<PiProviderProtocol, "api">): { format: string; effort: boolean } {
  const url = baseUrl.toLowerCase();
  const is = (ids: string[], hosts: string[]) =>
    ids.includes(providerId) || hosts.some((host) => url.includes(host));
  const isDeepSeek = is(["deepseek"], ["deepseek.com"]);
  const isZai = is(["zai", "zai-coding-cn"], ["api.z.ai", "open.bigmodel.cn"]);
  const isTogether = is(["together"], ["api.together.ai", "api.together.xyz"]);
  const isAntLing = is(["ant-ling"], ["api.ant-ling.com"]);
  const isOpenRouter = is(["openrouter"], ["openrouter.ai"]);
  const detectedFormat = isDeepSeek
    ? "deepseek"
    : isZai
      ? "zai"
      : isTogether
        ? "together"
        : isAntLing
          ? "ant-ling"
          : isOpenRouter
            ? "openrouter"
            : "openai";
  const detectedEffort =
    !is(["xai"], ["api.x.ai"]) &&
    !isZai &&
    !is(["moonshotai", "moonshotai-cn"], ["api.moonshot."]) &&
    !isTogether &&
    !is(["cloudflare-ai-gateway"], ["gateway.ai.cloudflare.com"]) &&
    !is(["nvidia"], ["integrate.api.nvidia.com"]) &&
    !isAntLing;
  return {
    format:
      typeof compat.thinkingFormat === "string"
        ? compat.thinkingFormat
        : detectedFormat,
    effort:
      typeof compat.supportsReasoningEffort === "boolean"
        ? compat.supportsReasoningEffort
        : detectedEffort,
  };
}

/**
 * Pi 会不会把档位原样作为 `reasoning_effort` 发给上游。Pi 官方只在这种协议下用
 * effort 档位生成映射（`supportsDirectReasoningEffort`）：Responses 总是；Chat
 * Completions 看 `piCompletionsThinking`。Anthropic 等协议还需要额外的 compat，
 * 不生成。
 */
export function piSendsReasoningEffort(protocol: PiProviderProtocol): boolean {
  if (protocol.api === "openai-responses") return true;
  if (protocol.api !== "openai-completions") return false;
  const { format, effort } = piCompletionsThinking(protocol);
  return format === "openai" && effort;
}

/**
 * 预设映射能否用在这一行：协议必须相同，预设显式写的 compat 不能与用户已写的值
 * 冲突；Chat Completions 还要求补上缺的 compat 之后，Pi 探测出的思考格式和是否
 * 发送 `reasoning_effort` 与预设一致（预设可能依赖 Pi 按地址探测的默认值）。
 * 能用时返回映射和需要补进模型的 compat。
 */
export function piPresetThinkingFor(
  piThinking: KnownModelMetadata["piThinking"],
  effective: PiProviderProtocol,
): { map: PiThinkingLevelMap; missingCompat: Record<string, unknown> } | null {
  if (!piThinking || piThinking.api !== effective.api) {
    return null;
  }
  const missingCompat: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(piThinking.compat ?? {})) {
    if (effective.compat[key] === undefined) missingCompat[key] = value;
    else if (JSON.stringify(effective.compat[key]) !== JSON.stringify(value)) {
      return null;
    }
  }
  if (piThinking.api === "openai-completions") {
    const preset = piCompletionsThinking({
      baseUrl: piThinking.baseUrl,
      providerId: "",
      compat: piThinking.compat ?? {},
    });
    const actual = piCompletionsThinking({
      ...effective,
      compat: { ...effective.compat, ...missingCompat },
    });
    if (preset.format !== actual.format || preset.effort !== actual.effort) {
      return null;
    }
  }
  return { map: { ...piThinking.thinkingLevelMap }, missingCompat };
}
