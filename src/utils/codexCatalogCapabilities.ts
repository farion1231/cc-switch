import type { CodexCatalogModel } from "@/types";

type CapabilityKey =
  | "supportsParallelToolCalls"
  | "inputModalities"
  | "baseInstructions"
  | "applyPatchToolType"
  | "webSearchToolType"
  | "supportsSearchTool"
  | "supportVerbosity"
  | "defaultVerbosity"
  | "supportedReasoningLevels"
  | "defaultReasoningLevel"
  | "truncationPolicy"
  | "multiAgentVersion"
  | "minimalClientVersion";

export type CodexCatalogCapabilities = Pick<CodexCatalogModel, CapabilityKey>;

const asRecord = (value: unknown): Record<string, unknown> | undefined =>
  typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;

const readAlias = (
  source: Record<string, unknown>,
  camelCase: string,
  snakeCase: string,
): unknown => source[camelCase] ?? source[snakeCase];

const normalizeString = (value: unknown): string | undefined => {
  if (typeof value !== "string") return undefined;
  const normalized = value.trim();
  return normalized || undefined;
};

/**
 * 统一解析模型目录中的隐藏能力字段。
 * 数据库使用 camelCase，Codex 实时目录反解时可能提供 snake_case。
 */
export function normalizeCodexCatalogCapabilities(
  value: unknown,
): CodexCatalogCapabilities {
  const source = asRecord(value) ?? {};
  const supportsParallelToolCalls = readAlias(
    source,
    "supportsParallelToolCalls",
    "supports_parallel_tool_calls",
  );
  const rawInputModalities = readAlias(
    source,
    "inputModalities",
    "input_modalities",
  );
  const inputModalities = Array.isArray(rawInputModalities)
    ? rawInputModalities.flatMap((item) => {
        const modality = normalizeString(item);
        return modality ? [modality] : [];
      })
    : [];
  const baseInstructions = normalizeString(
    readAlias(source, "baseInstructions", "base_instructions"),
  );
  const rawApplyPatchToolType = readAlias(
    source,
    "applyPatchToolType",
    "apply_patch_tool_type",
  );
  const applyPatchToolType =
    rawApplyPatchToolType === "freeform" ? "freeform" : undefined;
  const rawWebSearchToolType = readAlias(
    source,
    "webSearchToolType",
    "web_search_tool_type",
  );
  const webSearchToolType =
    rawWebSearchToolType === "text" || rawWebSearchToolType === "text_and_image"
      ? rawWebSearchToolType
      : undefined;
  const supportsSearchTool = readAlias(
    source,
    "supportsSearchTool",
    "supports_search_tool",
  );
  const supportVerbosity = readAlias(
    source,
    "supportVerbosity",
    "support_verbosity",
  );
  const defaultVerbosity = normalizeString(
    readAlias(source, "defaultVerbosity", "default_verbosity"),
  );
  const rawReasoningLevels = readAlias(
    source,
    "supportedReasoningLevels",
    "supported_reasoning_levels",
  );
  const supportedReasoningLevels = Array.isArray(rawReasoningLevels)
    ? rawReasoningLevels.flatMap((item) => {
        const level = asRecord(item);
        const effort = normalizeString(level?.effort);
        const description = normalizeString(level?.description);
        return effort && description ? [{ effort, description }] : [];
      })
    : [];
  const defaultReasoningLevel = normalizeString(
    readAlias(source, "defaultReasoningLevel", "default_reasoning_level"),
  );
  const rawTruncationPolicy = asRecord(
    readAlias(source, "truncationPolicy", "truncation_policy"),
  );
  const rawTruncationMode = rawTruncationPolicy?.mode;
  const rawTruncationLimit = rawTruncationPolicy?.limit;
  const truncationPolicy: CodexCatalogModel["truncationPolicy"] =
    (rawTruncationMode === "tokens" || rawTruncationMode === "bytes") &&
    typeof rawTruncationLimit === "number" &&
    Number.isInteger(rawTruncationLimit) &&
    rawTruncationLimit > 0
      ? { mode: rawTruncationMode, limit: rawTruncationLimit }
      : undefined;
  const multiAgentVersion = normalizeString(
    readAlias(source, "multiAgentVersion", "multi_agent_version"),
  );
  const minimalClientVersion = normalizeString(
    readAlias(source, "minimalClientVersion", "minimal_client_version"),
  );

  return {
    ...(typeof supportsParallelToolCalls === "boolean"
      ? { supportsParallelToolCalls }
      : {}),
    ...(inputModalities.length > 0 ? { inputModalities } : {}),
    ...(baseInstructions ? { baseInstructions } : {}),
    ...(applyPatchToolType ? { applyPatchToolType } : {}),
    ...(webSearchToolType ? { webSearchToolType } : {}),
    ...(typeof supportsSearchTool === "boolean" ? { supportsSearchTool } : {}),
    ...(typeof supportVerbosity === "boolean" ? { supportVerbosity } : {}),
    ...(defaultVerbosity ? { defaultVerbosity } : {}),
    ...(supportedReasoningLevels.length > 0
      ? { supportedReasoningLevels }
      : {}),
    ...(defaultReasoningLevel ? { defaultReasoningLevel } : {}),
    ...(truncationPolicy ? { truncationPolicy } : {}),
    ...(multiAgentVersion ? { multiAgentVersion } : {}),
    ...(minimalClientVersion ? { minimalClientVersion } : {}),
  };
}

export function codexCatalogCapabilitiesEqual(
  left: unknown,
  right: unknown,
): boolean {
  return (
    JSON.stringify(normalizeCodexCatalogCapabilities(left)) ===
    JSON.stringify(normalizeCodexCatalogCapabilities(right))
  );
}
