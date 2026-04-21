import type { FailoverRetryMode, FailoverRetryPolicy } from "@/types";

export interface NormalizedFailoverRetryPolicy {
  mode: FailoverRetryMode;
  maxRetries: number;
  baseDelaySeconds: number;
  maxDelaySeconds: number;
  backoffMultiplier: number;
  nonRetryableKeywords: string[];
}

export const DEFAULT_FAILOVER_RETRY_NON_RETRYABLE_KEYWORDS = [
  "invalid_api_key",
  "invalid_request",
  "context_length_exceeded",
] as const;

export const DEFAULT_FAILOVER_RETRY_POLICY: NormalizedFailoverRetryPolicy = {
  mode: "finite",
  maxRetries: 0,
  baseDelaySeconds: 3,
  maxDelaySeconds: 30,
  backoffMultiplier: 2,
  nonRetryableKeywords: [...DEFAULT_FAILOVER_RETRY_NON_RETRYABLE_KEYWORDS],
};

export function normalizeFailoverRetryPolicy(
  policy?: FailoverRetryPolicy | null,
): NormalizedFailoverRetryPolicy {
  const mode = policy?.mode === "infinite" ? "infinite" : "finite";
  const maxRetries = clampInteger(
    policy?.maxRetries,
    DEFAULT_FAILOVER_RETRY_POLICY.maxRetries,
    0,
  );
  const baseDelaySeconds = clampInteger(
    policy?.baseDelaySeconds,
    DEFAULT_FAILOVER_RETRY_POLICY.baseDelaySeconds,
    1,
  );
  const maxDelaySeconds = Math.max(
    clampInteger(
      policy?.maxDelaySeconds,
      DEFAULT_FAILOVER_RETRY_POLICY.maxDelaySeconds,
      1,
    ),
    baseDelaySeconds,
  );
  const backoffMultiplier = clampDecimal(
    policy?.backoffMultiplier,
    DEFAULT_FAILOVER_RETRY_POLICY.backoffMultiplier,
    1,
  );
  const nonRetryableKeywords = normalizeFailoverRetryKeywords(
    policy?.nonRetryableKeywords,
    DEFAULT_FAILOVER_RETRY_NON_RETRYABLE_KEYWORDS,
  );

  return {
    mode,
    maxRetries,
    baseDelaySeconds,
    maxDelaySeconds,
    backoffMultiplier,
    nonRetryableKeywords,
  };
}

export function isInfiniteFailoverRetry(
  policy?: FailoverRetryPolicy | null,
): boolean {
  return normalizeFailoverRetryPolicy(policy).mode === "infinite";
}

export function normalizeFailoverRetryKeyword(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[\s_-]+/g, "");
}

export function normalizeFailoverRetryKeywords(
  keywords?: string[] | null,
  fallback: readonly string[] = DEFAULT_FAILOVER_RETRY_NON_RETRYABLE_KEYWORDS,
): string[] {
  if (keywords === undefined || keywords === null) {
    return [...fallback];
  }

  const result: string[] = [];
  const seen = new Set<string>();

  for (const keyword of keywords) {
    if (typeof keyword !== "string") continue;
    const trimmed = keyword.trim();
    if (!trimmed) continue;

    const normalized = normalizeFailoverRetryKeyword(trimmed);
    if (!normalized || seen.has(normalized)) continue;

    seen.add(normalized);
    result.push(trimmed);
  }

  return result;
}

function clampInteger(
  value: number | undefined,
  fallback: number,
  min: number,
): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return fallback;
  }
  return Math.max(Math.floor(value), min);
}

function clampDecimal(
  value: number | undefined,
  fallback: number,
  min: number,
): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return fallback;
  }
  return Math.max(Number(value), min);
}
