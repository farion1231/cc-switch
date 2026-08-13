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
  messageKey:
    | "dsh.validation.modelRequired"
    | "dsh.validation.modelIdRequired"
    | "dsh.validation.modelIdDuplicate"
    | "dsh.validation.positiveInteger";
  field?: "contextWindow" | "maxTokens";
}

/** Validate model ids and positive capacity overrides. */
export function validateDshModels(
  models: readonly DshModel[],
  requireOne = true,
): DshModelValidationFailure | undefined {
  if (requireOne && models.length === 0) {
    return { index: 0, messageKey: "dsh.validation.modelRequired" };
  }
  const seen = new Set<string>();
  for (const [index, model] of models.entries()) {
    const id = model.id.trim();
    if (!id) return { index, messageKey: "dsh.validation.modelIdRequired" };
    if (seen.has(id))
      return { index, messageKey: "dsh.validation.modelIdDuplicate" };
    seen.add(id);
    for (const [field, value] of [
      ["contextWindow", model.contextWindow],
      ["maxTokens", model.maxTokens],
    ] as const) {
      if (value !== undefined && (!Number.isSafeInteger(value) || value <= 0)) {
        return {
          index,
          messageKey: "dsh.validation.positiveInteger",
          field,
        };
      }
    }
  }
  return undefined;
}

/** Validate a route id without exposing a backend regular expression. */
export function validateDshRoute(
  route: string,
): "dsh.validation.routeRequired" | "dsh.validation.routeFormat" | undefined {
  const value = route.trim();
  if (!value) return "dsh.validation.routeRequired";
  if (!DSH_ROUTE_PATTERN.test(value)) {
    return "dsh.validation.routeFormat";
  }
  return undefined;
}

/** Validate a one-shot API-key field. Empty means keep/no credential. */
export function validateDshApiKey(
  value: string,
):
  | "dsh.validation.apiKeyWhitespace"
  | "dsh.validation.apiKeyQuoted"
  | "dsh.validation.apiKeyFormat"
  | undefined {
  if (value.length === 0) return undefined;
  const trimmed = value.trim();
  if (!trimmed) return "dsh.validation.apiKeyWhitespace";
  const first = trimmed[0];
  if (
    (first === "'" || first === '"' || first === "`") &&
    trimmed.length > 1 &&
    trimmed.endsWith(first)
  ) {
    return "dsh.validation.apiKeyQuoted";
  }
  if (ENV_LINE.test(trimmed) || !LEGAL_API_KEY.test(trimmed)) {
    return "dsh.validation.apiKeyFormat";
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
