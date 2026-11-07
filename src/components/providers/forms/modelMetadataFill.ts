// 从拉取列表选中模型后，把查到的已知参数换成各应用的字段格式补进这一行。
// 统一规则：只补空着的字段，用户填过的值一律不动；布尔和模态只往「支持」方向补。

import type { KnownModelMetadata } from "@/lib/modelMetadata";
import type {
  ClaudeStackModel,
  CodexCatalogModel,
  OpenClawModel,
  OpenCodeModel,
} from "@/types";
import type { HermesModel } from "@/config/hermesProviderPresets";
import type { KimiProviderConfig } from "@/config/kimiProviderPresets";
import {
  PI_THINKING_LEVELS,
  type PiThinkingLevel,
  type PiThinkingLevelMap,
} from "@/config/piThinkingProfiles";

/** 补全前后是否不同：没补上任何字段时不提示用户。 */
export const metadataFilledAnything = (before: unknown, after: unknown) =>
  JSON.stringify(before) !== JSON.stringify(after);

const isBlank = (value: unknown) =>
  value === undefined || value === null || String(value).trim() === "";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

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
  const currentName =
    typeof row.displayName === "string" ? row.displayName.trim() : "";
  if (
    metadata.displayName &&
    (!currentName || currentName === row.model.trim())
  ) {
    next.displayName = metadata.displayName;
  }
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
  // 只补「支持图片」：没写时后端按注册表判断、认不出就当支持；查到只收文字就写
  // ["text"] 会关掉 Codex 的图片入口，而编辑界面又没有改回来的地方（#8682）。
  if (!row.inputModalities && metadata.inputModalities?.includes("image")) {
    next.inputModalities = textImageModalities(metadata.inputModalities);
  }
  return next;
}

/**
 * Claude Code 模型列表的一行：补显示名称；上游窗口达到 1M 时勾上 1M。
 * 用户明确取消过 1M（`oneM === false`）时不改回去。
 */
export function fillClaudeStackModel<T extends ClaudeStackModel>(
  row: T,
  metadata: KnownModelMetadata,
): T {
  const next = { ...row };
  const currentName =
    typeof row.displayName === "string" ? row.displayName.trim() : "";
  if (
    metadata.displayName &&
    (!currentName || currentName === row.model.trim())
  ) {
    next.displayName = metadata.displayName;
  }
  if (
    row.oneM === undefined &&
    metadata.contextWindow !== undefined &&
    metadata.contextWindow >= 1_000_000
  ) {
    next.oneM = true;
  }
  return next;
}

export function fillOpenClawModel(
  model: OpenClawModel,
  metadata: KnownModelMetadata,
): OpenClawModel {
  const next = { ...model };
  if (
    metadata.displayName &&
    (isBlank(model.name) || model.name.trim() === model.id.trim())
  ) {
    next.name = metadata.displayName;
  }
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
  if (
    metadata.reasoningEfforts?.length &&
    model.compat?.supportsReasoningEffort !== false
  ) {
    const compat = { ...(model.compat ?? {}) };
    if (compat.supportsReasoningEffort === undefined) {
      compat.supportsReasoningEffort = true;
    }
    if (!compat.supportedReasoningEfforts?.length) {
      compat.supportedReasoningEfforts = [...metadata.reasoningEfforts];
    }
    next.compat = compat;
  }
  return next;
}

export function fillHermesModel(
  model: HermesModel,
  metadata: KnownModelMetadata,
): HermesModel {
  const next = { ...model };
  if (
    metadata.displayName &&
    (isBlank(model.name) || model.name?.trim() === model.id.trim())
  ) {
    next.name = metadata.displayName;
  }
  if (isBlank(model.context_length) && metadata.contextWindow) {
    next.context_length = metadata.contextWindow;
  }
  // 只补 true：没写时 Hermes 自己查目录，用户明确写的 false 也不动。
  if (
    model.supports_vision === undefined &&
    metadata.inputModalities?.includes("image")
  ) {
    next.supports_vision = true;
  }
  return next;
}

/**
 * 把 models.dev 的 effort 档位换成 OpenCode 的 `variants`。不同 provider 包读的
 * 字段不同：OpenAI 系用 `reasoningEffort`，Anthropic 用 `effort`，Google 用
 * `thinkingConfig.thinkingLevel`。未知包按 OpenAI 兼容处理（本地网关多为此类）。
 */
export function opencodeVariantsFromEfforts(
  efforts: readonly string[] | undefined,
  npm: string | undefined,
): Record<string, Record<string, unknown>> | undefined {
  if (!efforts?.length) return undefined;
  const settingsFor = (effort: string): Record<string, unknown> | undefined => {
    if (npm === "@ai-sdk/anthropic") return { effort };
    if (npm === "@ai-sdk/google") {
      return {
        thinkingConfig: { includeThoughts: true, thinkingLevel: effort },
      };
    }
    // Bedrock 的档位字段没有统一写法，留给用户手填。
    if (npm === "@ai-sdk/amazon-bedrock") return undefined;
    return { reasoningEffort: effort };
  };
  const variants: Record<string, Record<string, unknown>> = {};
  for (const effort of efforts) {
    const settings = settingsFor(effort);
    if (settings) variants[effort] = settings;
  }
  return Object.keys(variants).length > 0 ? variants : undefined;
}

/** 只补 OpenCode `cost` 里缺的数字，用户写的值和额外字段原样保留。 */
function opencodeCostFromMetadata(
  current: unknown,
  metadata: Pick<KnownModelMetadata, "cost" | "costOver200k">,
): Record<string, unknown> | undefined {
  const next: Record<string, unknown> = isRecord(current) ? { ...current } : {};
  const cost = metadata.cost;
  if (cost) {
    if (typeof next.input !== "number") next.input = cost.input;
    if (typeof next.output !== "number") next.output = cost.output;
    if (cost.cacheRead !== undefined && typeof next.cache_read !== "number") {
      next.cache_read = cost.cacheRead;
    }
    if (cost.cacheWrite !== undefined && typeof next.cache_write !== "number") {
      next.cache_write = cost.cacheWrite;
    }
  }
  const over200k = metadata.costOver200k;
  if (over200k) {
    const existing: Record<string, unknown> = isRecord(next.context_over_200k)
      ? { ...next.context_over_200k }
      : {};
    if (typeof existing.input !== "number") existing.input = over200k.input;
    if (typeof existing.output !== "number") existing.output = over200k.output;
    if (
      over200k.cacheRead !== undefined &&
      typeof existing.cache_read !== "number"
    ) {
      existing.cache_read = over200k.cacheRead;
    }
    if (
      over200k.cacheWrite !== undefined &&
      typeof existing.cache_write !== "number"
    ) {
      existing.cache_write = over200k.cacheWrite;
    }
    next.context_over_200k = existing;
  }
  return Object.keys(next).length > 0 ? next : undefined;
}

export function fillOpenCodeModel(
  model: OpenCodeModel,
  metadata: KnownModelMetadata,
  options: {
    npm?: string;
    dialect?: "opencode" | "dsh" | "mcode";
    /** 这一行的模型 ID；显示名还等于它时视为占位值。 */
    modelId?: string;
  } = {},
): OpenCodeModel {
  const next = { ...model };
  const currentName = typeof model.name === "string" ? model.name.trim() : "";
  if (
    metadata.displayName &&
    (!currentName ||
      (options.modelId !== undefined && currentName === options.modelId.trim()))
  ) {
    next.name = metadata.displayName;
  }
  const limit = { ...(model.limit ?? {}) };
  if (isBlank(limit.context) && metadata.contextWindow) {
    limit.context = metadata.contextWindow;
  }
  if (isBlank(limit.output) && metadata.maxOutputTokens) {
    limit.output = metadata.maxOutputTokens;
  }
  if (
    options.dialect !== "dsh" &&
    isBlank(limit.input) &&
    metadata.maxInputTokens
  ) {
    limit.input = metadata.maxInputTokens;
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

  // DSH 走 pi-ai 适配器，档位写在 reasoningEfforts 而不是 variants。
  if (options.dialect === "dsh") {
    if (model.reasoningEfforts === undefined && metadata.reasoningEfforts) {
      const map = piThinkingLevelMapFromEfforts(metadata.reasoningEfforts);
      if (map) {
        next.reasoningEfforts = Object.fromEntries(
          Object.entries(map).filter(([, value]) => value !== null),
        );
      }
    }
    return next;
  }

  // MiniMax Code 不读 variants，思考档位写在 thinking.effortOptions。
  if (options.dialect === "mcode") {
    if (model.thinking === undefined && metadata.reasoningEfforts) {
      const efforts = metadata.reasoningEfforts.filter(
        (effort) => effort !== "none" && effort !== "off",
      );
      if (efforts.length > 0) next.thinking = { effortOptions: efforts };
    }
    return next;
  }

  if (metadata.family && isBlank(model.family)) {
    next.family = metadata.family;
  }
  if (metadata.releaseDate && isBlank(model.release_date)) {
    next.release_date = metadata.releaseDate;
  }
  if (metadata.status && isBlank(model.status)) {
    next.status = metadata.status;
  }
  if (metadata.attachment !== undefined && model.attachment === undefined) {
    next.attachment = metadata.attachment;
  }
  if (metadata.temperature !== undefined && model.temperature === undefined) {
    next.temperature = metadata.temperature;
  }
  if (metadata.toolCall !== undefined && model.tool_call === undefined) {
    next.tool_call = metadata.toolCall;
  }
  if (metadata.interleaved !== undefined && model.interleaved === undefined) {
    next.interleaved = metadata.interleaved;
  }
  if (
    (metadata.cost || metadata.costOver200k) &&
    (model.cost === undefined || isRecord(model.cost))
  ) {
    const cost = opencodeCostFromMetadata(model.cost, metadata);
    if (cost) next.cost = cost;
  }
  const currentVariants = model.variants;
  const hasVariants = isRecord(currentVariants)
    ? Object.keys(currentVariants).length > 0
    : currentVariants !== undefined;
  if (!hasVariants) {
    const variants = opencodeVariantsFromEfforts(
      metadata.reasoningEfforts,
      options.npm,
    );
    if (variants) next.variants = variants;
  }
  return next;
}

type KimiModel = KimiProviderConfig["models"][string];

/**
 * Kimi 的 `max_context_size` 是必填项，新加的行先带一个占位值；`replaceContext`
 * 为真表示这一行的窗口还是占位值、可以换成查到的值。能力只往「支持」方向补，
 * 和 Kimi 按目录导入模型时的取法一致（`image_in` 看输入模态，`thinking` 看
 * reasoning）。
 */
export function fillKimiModel(
  model: KimiModel,
  metadata: KnownModelMetadata,
  { replaceContext }: { replaceContext: boolean },
): KimiModel {
  const next = { ...model };
  if (
    metadata.displayName &&
    (isBlank(model.display_name) ||
      model.display_name?.trim() === model.model.trim())
  ) {
    next.display_name = metadata.displayName;
  }
  if (replaceContext && metadata.contextWindow) {
    next.max_context_size = metadata.contextWindow;
  }
  const capabilities = new Set(model.capabilities ?? []);
  if (metadata.inputModalities?.includes("image")) capabilities.add("image_in");
  if (metadata.reasoning === true) capabilities.add("thinking");
  if (capabilities.size > (model.capabilities?.length ?? 0)) {
    next.capabilities = [...capabilities];
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
 * 网关声明的档位优先：预设映射里上游值不在网关列表中的档位改成 `null`，避免
 * Pi 继续提供网关未声明支持的档位。
 */
export function restrictPiThinkingToEfforts(
  map: PiThinkingLevelMap,
  efforts: readonly string[],
): PiThinkingLevelMap {
  const supported = new Set(efforts);
  const next = { ...map };
  for (const [level, value] of Object.entries(next) as [
    PiThinkingLevel,
    string | null | undefined,
  ][]) {
    if (typeof value === "string" && !supported.has(value)) next[level] = null;
  }
  return next;
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
