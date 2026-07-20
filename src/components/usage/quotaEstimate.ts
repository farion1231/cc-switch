/**
 * Quota-estimate probe (read-only, in-memory only, no upstream logic changes).
 *
 * Providers only expose a used-percentage per subscription window
 * (`QuotaTier.utilization`), never an absolute quota. cc-switch already derives
 * the real dollar spend for each window from local session files
 * (`getModelStats(start, now, appType)`), so we can reverse-compute the
 * window's total dollar value.
 *
 * The subtlety: `utilization` (U) is ACCOUNT-wide — it counts every surface on
 * the account (this device, other machines, claude.ai chat, ...), while the
 * local spend (C) only covers what this install recorded. A naive cumulative
 * ratio `100·C/U` therefore underestimates whenever the account is used beyond
 * this device, and the historical baseline (e.g. a window already at 50% on a
 * fresh install) poisons it persistently.
 *
 * So the primary estimator is INCREMENTAL: accumulate ΔC and ΔU between
 * refreshes and take the slope `100·ΣΔC/ΣΔU`. The historical baseline cancels
 * in the deltas, so the estimate is robust to off-device usage — it converges
 * toward the true quota as you keep using THIS device. The cumulative ratio is
 * kept as a lower-bound correction (all local data is a floor the true quota
 * can't fall below); the shown value is `max(delta, cumulative)`.
 *
 * State lives in a ref and is dropped on unmount (session close) — nothing is
 * persisted, so a stale window never lingers; it re-converges on reopen.
 *
 * Depends only on stable contracts: `QuotaTier.utilization`, `resetsAt`, and
 * `usageApi.getModelStats`. No backend or SubscriptionQuota data-flow changes.
 */
import { useEffect, useRef, useState } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import { usageApi } from "@/lib/api/usage";
import type { ModelStats } from "@/types/usage";
import type { QuotaTier, SubscriptionQuota } from "@/types/subscription";

const HOUR_MS = 3_600_000;
const DAY_MS = 86_400_000;

/**
 * ── Tuning knobs ──────────────────────────────────────────────────────────
 * These trade responsiveness against stability. C (local spend) and U
 * (provider percentage) come from two unsynchronized sources and jitter by
 * ~1-2%, so most lean toward "steady". Each can be adjusted in isolation:
 *
 * - MIN_OBSERVED_DU — gate (percentage points). Hold the estimate hidden until
 *   this much window movement (ΣΔU) has been observed THIS session, so the
 *   delta slope rests on enough signal. 5-10 is a good range.
 * - RESET_DROP_PCT — a utilization drop larger than this is treated as a window
 *   reset: the delta accumulators start a fresh segment.
 * - EMA_ALPHA — smoothing weight of each new sample (0..1). Higher = snappier
 *   but jumpier; lower = steadier but slower. An EMA at alpha a keeps ~1/a
 *   recent samples of "memory" (0.12 ≈ 8).
 * - DISPLAY_DEADBAND_RATIO — hysteresis on the shown number. Hold the last
 *   figure until the EMA drifts past this fraction, then snap to it.
 * - TREND_EPS_RATIO — dead zone for the trend arrow. Moves within this fraction
 *   of the baseline read as "flat" instead of up/down.
 * - PROVISIONAL_MIN_TOKENS — bootstrap gate, measured on the 7-day window using
 *   cache-inclusive tokens (realTotalTokens; input + output + cache read +
 *   cache creation). Before the delta method has warmed up, if that window
 *   already holds enough local token history, show the direct (cumulative)
 *   inference immediately — marked provisional — instead of hiding. The one
 *   decision is shared across all tiers so 5h and 7d light up together.
 */
export const MIN_OBSERVED_DU = 5;
const RESET_DROP_PCT = 20;
const EMA_ALPHA = 0.12;
const DISPLAY_DEADBAND_RATIO = 0.02;
const TREND_EPS_RATIO = 0.08;
export const PROVISIONAL_MIN_TOKENS = 500_000_000;

/** Only Claude / Codex are supported (other providers are out of scope for now). */
export type EstimableApp = "claude" | "codex";

export function isEstimableApp(
  appId: string | undefined,
): appId is EstimableApp {
  return appId === "claude" || appId === "codex";
}

/**
 * Infer a window length (ms) from the tier name. The window start is
 * `resetsAt - windowLength`, so this needs no backend `limit_window_seconds`
 * and stays purely front-end. Unknown windows return null (skip estimation).
 */
export function tierWindowMs(name: string): number | null {
  switch (name) {
    case "five_hour":
      return 5 * HOUR_MS;
    case "seven_day":
    case "seven_day_opus":
    case "seven_day_sonnet":
    case "weekly_limit":
      return 7 * DAY_MS;
    case "30_day":
    case "monthly":
      return 30 * DAY_MS;
    default:
      return null;
  }
}

/**
 * Which models feed a tier's numerator.
 *
 * Claude's 7-day window is split into per-model tiers (`seven_day_opus` /
 * `seven_day_sonnet`), each with its own utilization, so those track a single
 * model group. Shared windows (five_hour / seven_day / all Codex windows) sum
 * every model. Depends on the model-name string (containing "opus" / "sonnet")
 * — the loose match tolerates future naming.
 */
export function tierModelPredicate(name: string): (model: string) => boolean {
  if (name === "seven_day_opus") return (m) => /opus/i.test(m);
  if (name === "seven_day_sonnet") return (m) => /sonnet/i.test(m);
  return () => true;
}

function sumCost(rows: ModelStats[], pred: (m: string) => boolean): number {
  return rows
    .filter((r) => pred(r.model))
    .reduce((acc, r) => acc + (Number.parseFloat(r.totalCost) || 0), 0);
}

/**
 * Cumulative lower-bound estimate: 100 · spend ÷ percentage over all local
 * data in the window. Biased low when the account is used off this device, so
 * it only ever serves as a floor. Null when spend or percentage is non-positive.
 */
export function cumulativeQuotaUsd(
  costUsd: number,
  utilization: number,
): number | null {
  if (!(utilization > 0) || !Number.isFinite(costUsd) || costUsd <= 0) {
    return null;
  }
  return (costUsd / utilization) * 100;
}

/**
 * Incremental (baseline-robust) estimate: 100 · ΣΔC ÷ ΣΔU, available only once
 * at least `minObservedDU` of window movement has been accumulated. Because it
 * works on deltas, any historical / off-device baseline cancels out.
 */
export function deltaSlopeQuotaUsd(
  sumDC: number,
  sumDU: number,
  minObservedDU: number = MIN_OBSERVED_DU,
): number | null {
  if (!(sumDU >= minObservedDU) || !(sumDU > 0) || sumDC <= 0) return null;
  return (sumDC / sumDU) * 100;
}

/**
 * Combine the two estimators: the baseline-robust delta is the primary value,
 * corrected upward by the cumulative lower bound from all local data. Returns
 * null (hidden) until the delta estimate has passed its gate.
 */
export function combineEstimates(
  deltaEst: number | null,
  cumulativeEst: number | null,
): number | null {
  if (deltaEst == null) return null;
  if (cumulativeEst == null) return deltaEst;
  return Math.max(deltaEst, cumulativeEst);
}

/** Exponential moving average: fold a new sample into the running estimate. */
export function foldEma(prev: number | null, sample: number): number {
  if (prev == null || !Number.isFinite(prev)) return sample;
  return prev + EMA_ALPHA * (sample - prev);
}

/**
 * Apply the deadband: return `prevDisplay` unless `ema` has drifted past the
 * threshold, in which case snap to `ema`. Pure so it can be unit-tested.
 */
export function applyDeadband(prevDisplay: number | null, ema: number): number {
  if (prevDisplay == null || !(prevDisplay > 0)) return ema;
  if (Math.abs(ema - prevDisplay) > DISPLAY_DEADBAND_RATIO * prevDisplay) {
    return ema;
  }
  return prevDisplay;
}

/** Trend of the estimate vs a baseline. up = provider raised quota, down = provider cut it. */
export function classifyTrend(
  baseline: number | null,
  current: number,
): "up" | "down" | "flat" {
  if (baseline == null || !(baseline > 0)) return "flat";
  const diff = current - baseline;
  if (Math.abs(diff) <= TREND_EPS_RATIO * baseline) return "flat";
  return diff > 0 ? "up" : "down";
}

export interface TierEstimate {
  tierName: string;
  /** Deadbanded, EMA-smoothed quota ($) for display; null until shown. */
  quotaUsd: number | null;
  /** Window movement (ΣΔU, percentage points) observed this session so far. */
  observedDeltaUtil: number;
  utilization: number;
  trend: "up" | "down" | "flat" | null;
  loading: boolean;
}

interface TierAccum {
  /** Last observed local window spend and account utilization (for deltas). */
  prevC: number;
  prevU: number;
  /** Accumulated positive deltas this session (a fresh segment resets these). */
  sumDC: number;
  sumDU: number;
  /** Smoothed combined estimate; null until the gate is met. */
  ema: number | null;
  /** Deadbanded value actually shown. */
  display: number | null;
  /** EMA before folding the current sample, used as the trend baseline. */
  baseline: number | null;
  /** The quota.queriedAt already folded, to avoid folding the same snapshot twice. */
  lastQueriedAt: number;
}

/**
 * Compute a quota estimate for every estimable tier of a SubscriptionQuota.
 *
 * Returns `Map<tierName, TierEstimate>`. Pure derived state plus one ref that
 * accumulates deltas and converges across refreshes; it is dropped on unmount
 * (re-converges on reopen — nothing is persisted).
 */
export function useQuotaEstimates(
  appId: string | undefined,
  quota: SubscriptionQuota | undefined,
): Map<string, TierEstimate> {
  const enabled = isEstimableApp(appId) && Boolean(quota?.success);
  const queriedAt = quota?.queriedAt ?? 0;

  const tiers: QuotaTier[] = enabled ? (quota?.tiers ?? []) : [];
  const estimableTiers = tiers.filter(
    (t) => tierWindowMs(t.name) != null && Boolean(t.resetsAt),
  );

  const results = useQueries({
    queries: estimableTiers.map((t) => {
      const windowMs = tierWindowMs(t.name)!;
      // get_model_stats takes startDate/endDate as Unix seconds (see
      // resolveUsageRange); divide by 1000, otherwise passing millis pushes the
      // range far into the future and returns no spend → the numerator is 0.
      const start = Math.floor(
        (new Date(t.resetsAt as string).getTime() - windowMs) / 1000,
      );
      const end = Math.floor(Date.now() / 1000);
      return {
        // queriedAt in the key: only recompute when the quota is re-fetched,
        // avoiding render churn. start in the key: bust the cache when the
        // window start changes so a stale result is not reused.
        queryKey: ["quotaEstimate", appId, t.name, queriedAt, start] as const,
        queryFn: () =>
          usageApi.getModelStats(start, end, appId as EstimableApp),
        enabled,
        staleTime: 5 * 60 * 1000,
      };
    }),
  });

  // Provisional bootstrap gate: anchored on the 7-day window, using
  // cache-inclusive tokens (realTotalTokens) — get_model_stats.total_tokens
  // excludes cache reads/creation, which understates a heavy account by ~100x.
  const sevenDayTier = estimableTiers.find(
    (t) => tierWindowMs(t.name) === 7 * DAY_MS,
  );
  const sevenDayStart =
    sevenDayTier?.resetsAt != null
      ? Math.floor(
          (new Date(sevenDayTier.resetsAt).getTime() - 7 * DAY_MS) / 1000,
        )
      : null;
  const sevenDaySummary = useQuery({
    queryKey: ["quotaEstimateTokens", appId, queriedAt, sevenDayStart],
    queryFn: () =>
      usageApi.getUsageSummary(
        sevenDayStart as number,
        Math.floor(Date.now() / 1000),
        appId as EstimableApp,
      ),
    enabled: enabled && sevenDayStart != null,
    staleTime: 5 * 60 * 1000,
  });
  const provisionalOK =
    (sevenDaySummary.data?.realTotalTokens ?? 0) >= PROVISIONAL_MIN_TOKENS;

  const accumRef = useRef<Map<string, TierAccum>>(new Map());
  const [, bump] = useState(0);

  // Fold each new snapshot into the delta accumulators once it lands.
  const settleSignature = results
    .map((r) => r.dataUpdatedAt)
    .concat(sevenDaySummary.dataUpdatedAt)
    .join(",");
  useEffect(() => {
    if (!enabled) return;

    // Pre-pass: current spend/utilization per tier. `provisionalOK` (the shared
    // 7-day bootstrap gate, computed above) is applied to every tier so 5h and
    // 7d appear together rather than one at a time.
    const snaps = estimableTiers.map((t, i) => {
      const rows = results[i]?.data;
      if (!rows) return null;
      return {
        t,
        C: sumCost(rows, tierModelPredicate(t.name)),
        U: t.utilization,
      };
    });

    let changed = false;
    snaps.forEach((s) => {
      if (!s) return;
      const { t, C, U } = s;
      if (!(C > 0) || !(U > 0)) return; // nothing measurable yet

      const key = `${appId}:${t.name}`;
      const rec = accumRef.current.get(key);
      if (rec && rec.lastQueriedAt === queriedAt) return; // already folded

      // Accumulate deltas only when a previous point exists; the first
      // observation still yields a cold estimate below (it needs C/U only, not
      // a delta), so a single quota fetch already shows something.
      let sumDC = rec?.sumDC ?? 0;
      let sumDU = rec?.sumDU ?? 0;
      if (rec) {
        const dU = U - rec.prevU;
        const dC = C - rec.prevC;
        if (dU < -RESET_DROP_PCT) {
          // Window reset: start a fresh accumulation segment.
          sumDC = 0;
          sumDU = 0;
        } else if (dU > 0 && dC > 0) {
          // Progress driven by local activity — the only intervals we trust.
          // (dU>0 but dC<=0 means off-device-only usage; skip so it can't pollute.)
          sumDC += dC;
          sumDU += dU;
        }
      }

      // Usage inference (delta, baseline-robust) is primary; before it warms up,
      // bootstrap from the cold inference (cumulative) when local history is rich.
      const deltaEst = deltaSlopeQuotaUsd(sumDC, sumDU);
      const cumEst = cumulativeQuotaUsd(C, U);
      let instant: number | null = null;
      if (deltaEst != null) {
        instant = combineEstimates(deltaEst, cumEst);
      } else if (provisionalOK && cumEst != null) {
        instant = cumEst;
      }

      let ema = rec?.ema ?? null;
      let display = rec?.display ?? null;
      let baseline = rec?.baseline ?? null;
      if (instant != null) {
        baseline = ema ?? instant;
        ema = foldEma(ema, instant);
        display = applyDeadband(display, ema);
      }
      accumRef.current.set(key, {
        prevC: C,
        prevU: U,
        sumDC,
        sumDU,
        ema,
        display,
        baseline,
        lastQueriedAt: queriedAt,
      });
      changed = true;
    });
    if (changed) bump((v) => v + 1);
    // settleSignature covers result-data changes; queriedAt covers snapshot rotation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, appId, queriedAt, settleSignature]);

  const estimates = new Map<string, TierEstimate>();
  estimableTiers.forEach((t, i) => {
    const rec = accumRef.current.get(`${appId}:${t.name}`);
    estimates.set(t.name, {
      tierName: t.name,
      quotaUsd: rec?.display ?? null,
      observedDeltaUtil: rec?.sumDU ?? 0,
      utilization: t.utilization,
      trend: rec?.ema != null ? classifyTrend(rec.baseline, rec.ema) : null,
      loading: results[i]?.isLoading ?? false,
    });
  });

  return estimates;
}
