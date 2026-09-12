import type { OpenCodeModel } from "@/types";
import type {
  ModelsDevCost,
  ModelsDevModel,
  ModelsDevReasoningOption,
  ModelsDevResponse,
} from "@/lib/modelsDevPricing";

// opencode 对 provider.models.<id> 执行 additionalProperties: false 严格校验，
// 超出白名单的字段（description、reasoning_options 等）会导致 opencode 拒绝加载配置。
// 机制结论来源：https://github.com/tttrove/opencode-model-fetch/docs/variant-mechanism.md
export const OPENCODE_MODEL_SCHEMA_ALLOWED_KEYS = [
  "id",
  "name",
  "family",
  "release_date",
  "attachment",
  "reasoning",
  "temperature",
  "tool_call",
  "interleaved",
  "cost",
  "limit",
  "modalities",
  "experimental",
  "status",
  "provider",
  "options",
  "headers",
  "variants",
] as const;

// 本功能会写入/刷新的能力字段；name/options/headers 与人工 extra 键不在其列。
const CAPABILITY_KEYS = [
  "family",
  "release_date",
  "attachment",
  "reasoning",
  "temperature",
  "tool_call",
  "modalities",
  "limit",
  "cost",
  "variants",
] as const;

// 官方源优先级：命中即用，避免抓到聚合网关的转述条目。
export const MODELS_DEV_SOURCE_PRIORITY = [
  "openai",
  "xai",
  "anthropic",
  "google",
  "google-vertex",
  "deepseek",
  "moonshotai",
  "zhipuai",
  "qwen",
  "minimax",
  "mistral",
  "meta",
  "amazon-bedrock",
  "azure",
] as const;

// OpenAI Responses、Azure 和 Bedrock Mantle 使用同一套 Responses 选项。
const RESPONSES_TRIO_NPM = new Set([
  "@ai-sdk/openai",
  "@ai-sdk/azure",
  "@ai-sdk/amazon-bedrock/mantle",
]);
const OPENAI_COMPATIBLE_NPM = "@ai-sdk/openai-compatible";
const ANTHROPIC_NPM = new Set([
  "@ai-sdk/anthropic",
  "@ai-sdk/google-vertex/anthropic",
]);
const BEDROCK_NPM = "@ai-sdk/amazon-bedrock";
const GOOGLE_NPM = new Set(["@ai-sdk/google", "@ai-sdk/google-vertex"]);

const VARIANT_INCLUDE = ["reasoning.encrypted_content"];
/** OpenCode 各 SDK 已知的全部思考档位（「添加档位」下拉展示全集，弱→强）。 */
export const ALL_EFFORT_LEVELS = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
] as const;
const WIDELY_SUPPORTED_EFFORTS = ["low", "medium", "high"];
const OPENAI_GPT5_1_EFFORTS = ["none", ...WIDELY_SUPPORTED_EFFORTS];
const OPENAI_GPT5_2_PLUS_EFFORTS = [...OPENAI_GPT5_1_EFFORTS, "xhigh"];
const OPENAI_GPT5_PRO_EFFORTS = ["high"];
const OPENAI_GPT5_CHAT_EFFORTS = ["medium"];
const OPENAI_GPT5_CODEX_XHIGH_EFFORTS = [...WIDELY_SUPPORTED_EFFORTS, "xhigh"];
const OPENAI_GPT5_CODEX_3_PLUS_EFFORTS = [
  "none",
  ...OPENAI_GPT5_CODEX_XHIGH_EFFORTS,
];

const DATE_NONE = "2025-11-13";
const DATE_XHIGH = "2025-12-04";
const GPT5_FAMILY_RE = /(?:^|\/)gpt-5(?:[.-]|$)/;
const GPT5_VERSION_RE = /(?:^|\/)gpt-5[.-](\d+)(?:[.-]|$)/;
const GPT5_PRO_RE = /(?:^|\/)gpt-5[.-]?pro(?:[.-]|$)/;
const GPT5_VERSIONED_PRO_RE = /(?:^|\/)gpt-5[.-]\d+[.-]pro(?:[.-]|$)/;

export interface ModelsDevEntryHit {
  providerId: string;
  entry: ModelsDevModel;
}

/** variants 档位键名风格：plain 纯名称；numbered 数字前缀（对配置 A-Z 重排免疫）。 */
export type ModelsDevVariantsStyle = "plain" | "numbered";

export interface BuildVariantsOptions {
  style?: ModelsDevVariantsStyle;
}

export interface ModelVariantsResult {
  variants: Record<string, Record<string, unknown>>;
  /** 成功生成的 variants 键名（numbered 风格下为实际写入的带序号键名） */
  efforts: string[];
  /** 被写入 {"disabled": true} 屏蔽的自动注入档位 */
  suppressed: string[];
}

/** numbered 风格键名：按声明顺序（弱→强）两位零填充编号，如 01-low。 */
export function numberedKey(index: number, effort: string): string {
  return `${String(index + 1).padStart(2, "0")}-${effort}`;
}

/** 扫描现有 variants 中 NN- 前缀的最大序号，返回追加编号后的新键名。 */
export function nextNumberedKey(
  variants: Record<string, unknown> | undefined,
  effort: string,
): string {
  let max = 0;
  for (const key of Object.keys(variants ?? {})) {
    const match = /^(\d{1,2})-/.exec(key);
    if (match) max = Math.max(max, Number(match[1]));
  }
  return numberedKey(max, effort);
}

export interface CapabilityFillSummary {
  efforts: string[];
  suppressed: string[];
  contextLimit: number | null;
}

export interface CapabilityFillResult {
  model: OpenCodeModel;
  summary: CapabilityFillSummary;
}

/** 在 models.dev 嵌套目录中查找模型：官方源优先级命中，未命中时大小写不敏感全表兜底。 */
export function findModelsDevEntry(
  catalog: ModelsDevResponse,
  modelId: string,
  forcedSource?: string,
): ModelsDevEntryHit | null {
  const sources = forcedSource ? [forcedSource] : MODELS_DEV_SOURCE_PRIORITY;
  for (const providerId of sources) {
    const entry = catalog[providerId]?.models?.[modelId];
    if (entry && typeof entry === "object") return { providerId, entry };
  }

  const target = modelId.toLowerCase();
  for (const [providerId, provider] of Object.entries(catalog)) {
    for (const [key, entry] of Object.entries(provider?.models ?? {})) {
      if (key.toLowerCase() === target && entry && typeof entry === "object") {
        return { providerId, entry };
      }
    }
  }
  return null;
}

function gpt5Version(modelId: string): number | undefined {
  return Number(GPT5_VERSION_RE.exec(modelId)?.[1]) || undefined;
}

function gpt5ChatReasoningEfforts(modelId: string): string[] | undefined {
  if (!GPT5_FAMILY_RE.test(modelId) || !modelId.includes("-chat")) {
    return undefined;
  }
  return gpt5Version(modelId) === undefined ? [] : OPENAI_GPT5_CHAT_EFFORTS;
}

function gpt5CodexReasoningEfforts(modelId: string): string[] | undefined {
  if (!GPT5_FAMILY_RE.test(modelId) || !modelId.includes("codex")) {
    return undefined;
  }
  const version = gpt5Version(modelId);
  if (version !== undefined && version >= 3) {
    return OPENAI_GPT5_CODEX_3_PLUS_EFFORTS;
  }
  if (
    modelId.includes("codex-max") ||
    (version !== undefined && version >= 2)
  ) {
    return OPENAI_GPT5_CODEX_XHIGH_EFFORTS;
  }
  return WIDELY_SUPPORTED_EFFORTS;
}

function versionedGpt5ReasoningEfforts(modelId: string): string[] | undefined {
  if (GPT5_VERSIONED_PRO_RE.test(modelId)) {
    return ["medium", "high", "xhigh"];
  }
  const version = gpt5Version(modelId);
  if (version === undefined) return undefined;
  if (version === 1) return OPENAI_GPT5_1_EFFORTS;
  return OPENAI_GPT5_2_PLUS_EFFORTS;
}

/** 复刻当前 OpenCode 对 OpenAI Responses 模型计算默认注入档位的规则。 */
function openaiReasoningEfforts(
  modelId: string,
  releaseDate?: string,
): string[] {
  const id = modelId.toLowerCase();
  if (id.includes("deep-research")) return ["medium"];
  const chatEfforts = gpt5ChatReasoningEfforts(id);
  if (chatEfforts) return chatEfforts;
  if (GPT5_PRO_RE.test(id)) return OPENAI_GPT5_PRO_EFFORTS;
  const codexEfforts = gpt5CodexReasoningEfforts(id);
  if (codexEfforts) return codexEfforts;
  const versionedEfforts = versionedGpt5ReasoningEfforts(id);
  if (versionedEfforts) return versionedEfforts;

  const efforts = [...WIDELY_SUPPORTED_EFFORTS];
  if (GPT5_FAMILY_RE.test(id)) efforts.unshift("minimal");
  if (releaseDate && releaseDate >= DATE_NONE) efforts.unshift("none");
  if (releaseDate && releaseDate >= DATE_XHIGH) efforts.push("xhigh");
  return efforts;
}

/** 保留这一导出供调用方和测试使用。 */
export function injectedEffortsForModel(
  modelId: string,
  releaseDate?: string,
): string[] {
  return openaiReasoningEfforts(modelId, releaseDate);
}

function anthropicUsesModernAdaptiveThinking(modelId: string): boolean {
  const id = modelId.toLowerCase();
  if (!id.includes("claude-")) return false;
  const version = /claude-(?:[a-z]+-)?(\d+)(?:[.-](\d{1,2}))?(?:[.@-]|$)/i.exec(
    id,
  );
  if (!version) return true;
  const major = Number(version[1]);
  const minor = Number(version[2] ?? 0);
  return major > 4 || (major === 4 && minor >= 7);
}

function anthropicOpus45(modelId: string): boolean {
  const id = modelId.toLowerCase();
  return ["opus-4-5", "opus-4.5"].some((value) => id.includes(value));
}

function isAnthropicBedrockModel(modelId: string): boolean {
  const id = modelId.toLowerCase();
  return id.includes("anthropic") || id.includes("claude-");
}

function anthropicAdaptiveEfforts(modelId: string): string[] | null {
  const id = modelId.toLowerCase();
  if (anthropicUsesModernAdaptiveThinking(id)) {
    return ["low", "medium", "high", "xhigh", "max"];
  }
  if (
    [
      "opus-4-6",
      "opus-4.6",
      "4-6-opus",
      "4.6-opus",
      "sonnet-4-6",
      "sonnet-4.6",
      "4-6-sonnet",
      "4.6-sonnet",
    ].some((value) => id.includes(value))
  ) {
    return ["low", "medium", "high", "max"];
  }
  return null;
}

function isKimiFamily(modelId: string): boolean {
  const id = modelId.toLowerCase();
  return id.includes("kimi") || id.includes("moonshot");
}

function anthropicBudgetTokens(outputLimit?: number): number {
  const calculated = Math.floor((outputLimit ?? 32_000) / 2 - 1);
  return Math.max(1, Math.min(16_000, calculated));
}

function anthropicEffortVariant(
  modelId: string,
  effort: string,
  outputLimit?: number,
): Record<string, unknown> {
  if (anthropicOpus45(modelId)) {
    return {
      thinking: {
        type: "enabled",
        budgetTokens: anthropicBudgetTokens(outputLimit),
      },
      effort,
    };
  }

  if (anthropicAdaptiveEfforts(modelId) || isKimiFamily(modelId)) {
    return {
      thinking: {
        type: "adaptive",
        ...(anthropicUsesModernAdaptiveThinking(modelId) ||
        isKimiFamily(modelId)
          ? { display: "summarized" }
          : {}),
      },
      effort,
    };
  }

  // This is the upstream OpenCode fallback for Anthropic-compatible effort
  // metadata. It is still provider-specific (not OpenAI reasoningEffort).
  return { effort };
}

function bedrockEffortVariant(
  modelId: string,
  effort: string,
  outputLimit?: number,
): Record<string, unknown> | undefined {
  if (anthropicAdaptiveEfforts(modelId)) {
    return {
      reasoningConfig: {
        type: "adaptive",
        maxReasoningEffort: effort,
        ...(anthropicUsesModernAdaptiveThinking(modelId)
          ? { display: "summarized" }
          : {}),
      },
    };
  }

  if (anthropicOpus45(modelId)) {
    return {
      reasoningConfig: {
        type: "enabled",
        budgetTokens: anthropicBudgetTokens(outputLimit),
        maxReasoningEffort: effort,
      },
    };
  }

  // The Bedrock SDK does not accept effort controls for older Anthropic
  // models; those models are represented by budget_tokens metadata instead.
  if (isAnthropicBedrockModel(modelId)) return undefined;

  return {
    reasoningConfig: {
      type: "enabled",
      maxReasoningEffort: effort,
    },
  };
}

function googleThinkingLevelEfforts(modelId: string): string[] {
  const id = modelId.toLowerCase();
  if (!id.includes("gemini-3")) return ["low", "high"];
  if (id.includes("flash-image")) return ["minimal", "high"];
  if (id.includes("pro-image")) return ["high"];
  if (id.includes("flash")) return ["minimal", "low", "medium", "high"];
  return ["low", "medium", "high"];
}

function isGlm52(modelId: string): boolean {
  const id = modelId.toLowerCase();
  return ["glm-5.2", "glm-5-2", "glm-5p2"].some((name) => id.includes(name));
}

function hasNoAutomaticVariants(modelId: string): boolean {
  const id = modelId.toLowerCase();
  return (
    id.includes("deepseek-chat") ||
    id.includes("deepseek-reasoner") ||
    id.includes("deepseek-r1") ||
    id.includes("deepseek-v3") ||
    id.includes("minimax") ||
    (id.includes("glm") && !isGlm52(id)) ||
    id.includes("kimi") ||
    id.includes("k2p") ||
    id.includes("qwen") ||
    id.includes("big-pickle")
  );
}

/** 复刻当前 OpenCode ProviderTransform.variants() 对五类 SDK 生成的默认档位键。 */
export function injectedEffortsForNpm(
  npm: string,
  modelId: string,
  releaseDate?: string,
): string[] | null {
  const id = modelId.toLowerCase();

  if (
    id.includes("minimax-m3") &&
    (npm === OPENAI_COMPATIBLE_NPM || ANTHROPIC_NPM.has(npm))
  ) {
    return ["none", "thinking"];
  }

  if (isGlm52(id)) {
    if (npm === OPENAI_COMPATIBLE_NPM || ANTHROPIC_NPM.has(npm)) {
      return ["high", "max"];
    }
  }

  if (isKimiFamily(id) && ANTHROPIC_NPM.has(npm)) {
    return ["low", "medium", "high", "xhigh", "max"];
  }

  if (hasNoAutomaticVariants(id)) return [];

  if (id.includes("grok-3-mini")) return ["low", "high"];

  if (RESPONSES_TRIO_NPM.has(npm)) {
    return openaiReasoningEfforts(modelId, releaseDate);
  }

  if (npm === OPENAI_COMPATIBLE_NPM) {
    if (id.includes("north-mini-code")) return ["none", "high"];
    const efforts = [...WIDELY_SUPPORTED_EFFORTS];
    if (id.includes("deepseek-v4")) efforts.push("max");
    return efforts;
  }

  if (ANTHROPIC_NPM.has(npm)) {
    return (
      anthropicAdaptiveEfforts(id) ??
      (anthropicOpus45(id) ? WIDELY_SUPPORTED_EFFORTS : ["high", "max"])
    );
  }

  if (npm === BEDROCK_NPM) {
    if (anthropicAdaptiveEfforts(id)) return anthropicAdaptiveEfforts(id);
    if (isAnthropicBedrockModel(id)) return ["high", "max"];
    return WIDELY_SUPPORTED_EFFORTS;
  }

  if (GOOGLE_NPM.has(npm)) {
    if (id.includes("2.5")) return ["high", "max"];
    return googleThinkingLevelEfforts(id);
  }

  return null;
}

/** 将一个 effort 值转换为选定 SDK 真正识别的 provider options。 */
function variantObject(
  effort: string,
  npm: string,
  modelId: string,
  outputLimit?: number,
): Record<string, unknown> | undefined {
  if (RESPONSES_TRIO_NPM.has(npm)) {
    return {
      reasoningEffort: effort,
      reasoningSummary: "auto",
      include: [...VARIANT_INCLUDE],
    };
  }
  if (npm === OPENAI_COMPATIBLE_NPM) return { reasoningEffort: effort };
  if (ANTHROPIC_NPM.has(npm)) {
    return anthropicEffortVariant(modelId, effort, outputLimit);
  }
  if (npm === BEDROCK_NPM) {
    return bedrockEffortVariant(modelId, effort, outputLimit);
  }
  if (GOOGLE_NPM.has(npm)) {
    return {
      thinkingConfig: {
        includeThoughts: true,
        thinkingLevel: effort,
      },
    };
  }
  return undefined;
}

/** 供「添加档位」UI 为单个 effort 生成正确 SDK 参数体；未知 SDK 返回 undefined。 */
export function buildVariantForEffort(
  npm: string,
  modelId: string,
  effort: string,
  outputLimit?: number,
): Record<string, unknown> | undefined {
  return variantObject(effort, npm, modelId, outputLimit);
}

function reasoningBudgetVariant(
  npm: string,
  budget: number,
): Record<string, unknown> | undefined {
  if (ANTHROPIC_NPM.has(npm)) {
    return { thinking: { type: "enabled", budgetTokens: budget } };
  }
  if (npm === BEDROCK_NPM) {
    return { reasoningConfig: { type: "enabled", budgetTokens: budget } };
  }
  if (GOOGLE_NPM.has(npm)) {
    return {
      thinkingConfig: { includeThoughts: true, thinkingBudget: budget },
    };
  }
  return undefined;
}

function normalizedReasoningOptions(
  entry: ModelsDevModel,
): ModelsDevReasoningOption[] {
  const options = entry.reasoning_options;
  if (!options) return [];
  return (Array.isArray(options) ? options : [options]).filter(
    (option): option is ModelsDevReasoningOption =>
      Boolean(option && typeof option === "object"),
  );
}

function effortValues(options: ModelsDevReasoningOption[]): string[] {
  const values: string[] = [];
  for (const option of options) {
    if (option.type !== "effort") continue;
    for (const value of option.values ?? []) {
      const normalized = value === null ? "none" : value;
      if (
        typeof normalized === "string" &&
        normalized.length > 0 &&
        !values.includes(normalized)
      ) {
        values.push(normalized);
      }
    }
  }
  return values;
}

function finiteNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value)
    ? value
    : undefined;
}

function budgetValues(
  options: ModelsDevReasoningOption[],
  npm: string,
  outputLimit?: number,
): Record<string, Record<string, unknown>> {
  const budget = options.find((option) => option.type === "budget_tokens");
  if (!budget) return {};

  const modelMaximum = finiteNumber(outputLimit);
  const declaredMaximum = finiteNumber(budget.max);
  const maximum = Math.floor(
    Math.min(
      declaredMaximum ?? 31_999,
      modelMaximum !== undefined ? modelMaximum - 1 : 31_999,
      31_999,
    ),
  );
  if (maximum <= 0) return {};

  const declaredMinimum = finiteNumber(budget.min) ?? 0;
  const high = Math.min(
    Math.max(declaredMinimum, Math.floor((maximum + 1) / 2)),
    maximum,
  );
  const variants: Record<string, Record<string, unknown>> = {};
  const highVariant = reasoningBudgetVariant(npm, high);
  const maxVariant = reasoningBudgetVariant(npm, maximum);
  if (highVariant) variants.high = highVariant;
  if (maxVariant) variants.max = maxVariant;
  return variants;
}

/**
 * 从 models.dev 的 reasoning_options 生成 variants。
 * effort、budget_tokens 和 SDK 参数名均按 OpenCode 当前转换规则处理；
 * 只有 toggle 或未知 SDK 时返回 null，让调用方保留原有配置。
 * style 为 numbered 时档位键按声明顺序（弱→强）加两位序号前缀，
 * 注入的 plain 档位会自动落入 suppressed（disabled）屏蔽。
 */
export function buildVariantsForModel(
  entry: ModelsDevModel,
  npm: string,
  modelId: string,
  outputLimit?: number,
  options?: BuildVariantsOptions,
): ModelVariantsResult | null {
  const style = options?.style ?? "plain";
  if (
    Array.isArray(entry.reasoning_options) &&
    entry.reasoning_options.length === 0
  ) {
    const injected =
      injectedEffortsForNpm(npm, modelId, entry.release_date) ?? [];
    return {
      variants: Object.fromEntries(
        injected.map((effort) => [effort, { disabled: true }]),
      ),
      efforts: [],
      suppressed: [...injected],
    };
  }

  const options_ = normalizedReasoningOptions(entry);
  if (options_.length === 0) return null;

  const declaredEfforts = effortValues(options_);
  const variants: Record<string, Record<string, unknown>> = {};
  const generatedKeys: string[] = [];

  // OpenCode gives effort metadata precedence over budget/toggle metadata.
  if (declaredEfforts.length > 0) {
    declaredEfforts.forEach((effort, index) => {
      const value = variantObject(effort, npm, modelId, outputLimit);
      if (value) {
        const key = style === "numbered" ? numberedKey(index, effort) : effort;
        variants[key] = value;
        generatedKeys.push(key);
      }
    });
  } else {
    const budget = budgetValues(options_, npm, outputLimit);
    Object.entries(budget).forEach(([effort, value], index) => {
      const key = style === "numbered" ? numberedKey(index, effort) : effort;
      variants[key] = value;
      generatedKeys.push(key);
    });
  }

  // toggle-only metadata has no safe, common payload for these SDKs.
  if (generatedKeys.length === 0) return null;

  const suppressed: string[] = [];
  const injected = injectedEffortsForNpm(npm, modelId, entry.release_date);
  for (const effort of injected ?? []) {
    if (!generatedKeys.includes(effort) && !(effort in variants)) {
      variants[effort] = { disabled: true };
      suppressed.push(effort);
    }
  }
  return { variants, efforts: generatedKeys, suppressed };
}

const COST_KEYS = ["input", "output", "cache_read", "cache_write"] as const;

function pickCostNumbers(source: unknown): Record<string, number> {
  const out: Record<string, number> = {};
  if (source && typeof source === "object") {
    for (const key of COST_KEYS) {
      const value = (source as Record<string, unknown>)[key];
      if (typeof value === "number") out[key] = value;
    }
  }
  return out;
}

// models.dev 的 cost.tiers 数组不可写入 opencode 配置，需转换为 context_over_200k。
function resolveContextOver200k(
  cost: ModelsDevCost,
): Record<string, number> | null {
  const direct = pickCostNumbers(cost.context_over_200k);
  if (Object.keys(direct).length > 0) return direct;
  for (const tier of cost.tiers ?? []) {
    if (tier?.tier?.type !== "context") continue;
    const converted = pickCostNumbers(tier);
    if (Object.keys(converted).length > 0) return converted;
  }
  return null;
}

/** 按 opencode schema 白名单裁剪 models.dev 条目，丢弃一切 schema 外字段。 */
export function transformModelsDevEntry(
  entry: ModelsDevModel,
): Record<string, unknown> {
  const out: Record<string, unknown> = {};

  const putString = (key: string, value: unknown) => {
    if (typeof value === "string" && value !== "") out[key] = value;
  };
  const putBoolean = (key: string, value: unknown) => {
    if (typeof value === "boolean") out[key] = value;
  };

  putString("name", entry.name);
  putString("family", entry.family);
  putString("release_date", entry.release_date);
  putBoolean("attachment", entry.attachment);
  putBoolean("reasoning", entry.reasoning);
  putBoolean("temperature", entry.temperature);
  putBoolean("tool_call", entry.tool_call);

  if (entry.modalities && typeof entry.modalities === "object") {
    out.modalities = entry.modalities;
  }

  const limit = entry.limit;
  if (limit && typeof limit === "object") {
    const next: Record<string, number> = {};
    for (const key of ["context", "input", "output"] as const) {
      const value = limit[key];
      if (typeof value === "number") next[key] = value;
    }
    if (Object.keys(next).length > 0) out.limit = next;
  }

  const cost = entry.cost;
  if (cost && typeof cost === "object") {
    const next: Record<string, number | Record<string, number>> =
      pickCostNumbers(cost);
    const over200k = resolveContextOver200k(cost);
    if (over200k) next.context_over_200k = over200k;
    if (Object.keys(next).length > 0) out.cost = next;
  }

  return out;
}

/**
 * 把 models.dev 条目的能力数据合并进现有模型配置：
 * - 能力字段（cost/modalities 等）以 models.dev 为准覆盖；
 * - limit.context / limit.output 用户已手填时保留；缺失字段才采用 models.dev 值；
 * - 显示名非空则保留人工输入，为空时填官方名；
 * - options/headers 与白名单外的人工自定义键原样保留；
 * - variants 只有在能按当前 SDK 安全生成时才替换，否则保留原值；
 * - 不写入 id（模型键名即 id，避免改键名后失同步）。
 */
export function applyModelsDevCapabilities(
  existing: OpenCodeModel,
  npm: string,
  modelId: string,
  entry: ModelsDevModel,
  options?: BuildVariantsOptions,
): CapabilityFillResult {
  const next: Record<string, unknown> = { ...existing };
  const existingVariants = existing.variants;
  for (const key of CAPABILITY_KEYS) delete next[key];

  const capabilities = transformModelsDevEntry(entry);
  const existingLimit =
    existing.limit && typeof existing.limit === "object"
      ? (existing.limit as {
          context?: number;
          input?: number;
          output?: number;
        })
      : {};
  const devLimit = capabilities.limit as Record<string, number> | undefined;
  const mergedLimit: Record<string, number> = { ...(devLimit ?? {}) };
  if (typeof existingLimit.context === "number") {
    mergedLimit.context = existingLimit.context;
  }
  if (typeof existingLimit.input === "number") {
    mergedLimit.input = existingLimit.input;
  }
  if (typeof existingLimit.output === "number") {
    mergedLimit.output = existingLimit.output;
  }
  if (Object.keys(mergedLimit).length > 0) {
    capabilities.limit = mergedLimit;
  } else {
    delete capabilities.limit;
  }
  Object.assign(next, capabilities);

  const outputLimit =
    next.limit && typeof next.limit === "object"
      ? (next.limit as { output?: number }).output
      : undefined;
  const variantsResult = buildVariantsForModel(
    entry,
    npm,
    modelId,
    outputLimit,
    options,
  );
  if (variantsResult) {
    next.variants = variantsResult.variants;
  } else if (existingVariants !== undefined) {
    next.variants = existingVariants;
  }

  if (typeof existing.name === "string" && existing.name.trim() !== "") {
    next.name = existing.name;
  }

  const limit = next.limit as { context?: number } | undefined;
  return {
    model: next as OpenCodeModel,
    summary: {
      efforts: variantsResult?.efforts ?? [],
      suppressed: variantsResult?.suppressed ?? [],
      contextLimit: typeof limit?.context === "number" ? limit.context : null,
    },
  };
}
