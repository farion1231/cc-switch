import type { DshModel } from "@/lib/api/dsh";

/** Protocols accepted by the DSH `llm-pi-ai` profile seam, in stable order. */
export const DSH_PROTOCOLS = [
  "openai-completions",
  "openai-responses",
  "anthropic-messages",
] as const;

/** Route ids are settings keys and may safely derive a POSIX credential name. */
export const DSH_ROUTE_PATTERN = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/;

/** Printable API-key characters accepted by the DSH client editor. */
const LEGAL_API_KEY = /^[\x21-\x7E]+$/;
const ENV_LINE = /^[A-Z][A-Z0-9_]*=[^=]/;

/** A model row validation result, named so the UI can focus the offending row. */
export interface DshModelValidationFailure {
  index: number;
  message: string;
}

/** Validate model ids and positive capacity overrides. */
export function validateDshModels(
  models: readonly DshModel[],
  requireOne = true,
): DshModelValidationFailure | undefined {
  if (requireOne && models.length === 0) {
    return { index: 0, message: "至少填写一个模型" };
  }
  const seen = new Set<string>();
  for (const [index, model] of models.entries()) {
    const id = model.id.trim();
    if (!id) return { index, message: "模型 ID 不能为空" };
    if (seen.has(id)) return { index, message: "模型 ID 不能重复" };
    seen.add(id);
    for (const [field, value] of [
      ["contextWindow", model.contextWindow],
      ["maxTokens", model.maxTokens],
    ] as const) {
      if (value !== undefined && (!Number.isSafeInteger(value) || value <= 0)) {
        return { index, message: `${field} 必须为正整数` };
      }
    }
  }
  return undefined;
}

/** Validate a route id without exposing a backend regular expression. */
export function validateDshRoute(route: string): string | undefined {
  const value = route.trim();
  if (!value) return "Provider ID 不能为空";
  if (!DSH_ROUTE_PATTERN.test(value)) {
    return "Provider ID 只能使用小写字母、数字和短横线，且必须以字母开头";
  }
  return undefined;
}

/** Validate a one-shot API-key field. Empty means keep/no credential. */
export function validateDshApiKey(value: string): string | undefined {
  if (value.length === 0) return undefined;
  const trimmed = value.trim();
  if (!trimmed) return "API key 不能只包含空白字符";
  const first = trimmed[0];
  if (
    (first === "'" || first === '"' || first === "`") &&
    trimmed.length > 1 &&
    trimmed.endsWith(first)
  ) {
    return "请粘贴未加引号的 API key";
  }
  if (ENV_LINE.test(trimmed) || !LEGAL_API_KEY.test(trimmed)) {
    return "API key 含有无效字符或环境变量赋值格式";
  }
  return undefined;
}

/** Derive the conventional DSH credential reference for a route. */
export function deriveDshCredentialRef(route: string): string {
  const normalized = route.toUpperCase().replace(/[^A-Z0-9]+/g, "_");
  return `${normalized}_API_KEY`;
}

/** Parse a positive capacity input, accepting decimal K/M suffixes. */
export function parseDshCapacity(value: string): number | undefined {
  const raw = value.trim();
  if (!raw) return undefined;
  const match = /^(\d+(?:\.\d+)?)([KMG])?$/i.exec(raw);
  if (!match) return undefined;
  const scale =
    match[2]?.toUpperCase() === "K"
      ? 1_000
      : match[2]?.toUpperCase() === "M"
        ? 1_000_000
        : match[2]?.toUpperCase() === "G"
          ? 1_000_000_000
          : 1;
  const parsed = Number(match[1]) * scale;
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : undefined;
}

/** Display a capacity without changing the stored numeric value. */
export function formatDshCapacity(value: number | undefined): string {
  return value === undefined ? "" : String(value);
}
