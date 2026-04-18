import type { FailoverRetryMode, FailoverRetryPolicy } from "@/types";

export interface NormalizedFailoverRetryPolicy {
  mode: FailoverRetryMode;
  maxRetries: number;
  baseDelaySeconds: number;
  maxDelaySeconds: number;
  backoffMultiplier: number;
}

export const DEFAULT_FAILOVER_RETRY_POLICY: NormalizedFailoverRetryPolicy = {
  mode: "finite",
  maxRetries: 1,
  baseDelaySeconds: 3,
  maxDelaySeconds: 30,
  backoffMultiplier: 2,
};

export function normalizeFailoverRetryPolicy(
  policy?: FailoverRetryPolicy | null,
): NormalizedFailoverRetryPolicy {
  const mode = policy?.mode === "infinite" ? "infinite" : "finite";
  const maxRetries = clampInteger(
    policy?.maxRetries,
    DEFAULT_FAILOVER_RETRY_POLICY.maxRetries,
    1,
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

  return {
    mode,
    maxRetries,
    baseDelaySeconds,
    maxDelaySeconds,
    backoffMultiplier,
  };
}

export function isInfiniteFailoverRetry(
  policy?: FailoverRetryPolicy | null,
): boolean {
  return normalizeFailoverRetryPolicy(policy).mode === "infinite";
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
