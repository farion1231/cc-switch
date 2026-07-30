/**
 * Hourly subscription-quota probe (additive — no upstream data-flow changes).
 *
 * The quota footer already fetches `get_subscription_quota`, but only while a
 * provider card for the current app is on screen, and it keeps nothing: once a
 * window rolls over its previous utilization is unrecoverable. This hook keeps
 * one long-lived observer per quota-capable app so a reading lands at least
 * once an hour, and persists each one through `record_quota_history`.
 *
 * It deliberately reuses `subscriptionKeys.quota(appId)` — the SAME react-query
 * key as the footer — so the two share a single in-flight request and cache
 * entry. While a card is visible the footer's 5-minute poll drives the fetches
 * and this hook costs nothing extra; when nothing is on screen the hourly timer
 * here takes over. The backend upserts per measured hour, so those extra
 * refreshes only sharpen the sample rather than duplicating rows.
 *
 * Apps whose credentials are absent short-circuit in the backend without a
 * network call, so probing a not-logged-in app is effectively free.
 */
import { useEffect } from "react";
import { useQueries, useQueryClient } from "@tanstack/react-query";
import { subscriptionApi } from "@/lib/api/subscription";
import { quotaHistoryApi } from "@/lib/api/quotaHistory";
import { subscriptionKeys } from "@/lib/query/subscription";
import { quotaHistoryKeys } from "@/lib/query/quotaHistory";
import type { AppId } from "@/lib/api/types";
import type { VisibleApps } from "@/types";
import { toQuotaSamples } from "@/components/usage/quotaHistory";

/** Apps `get_subscription_quota` supports (mirrors `useSubscriptionQuota`). */
export const PROBE_APPS = [
  "claude",
  "codex",
  "gemini",
] as const satisfies readonly AppId[];

export const PROBE_INTERVAL_MS = 60 * 60 * 1000;

export interface UseQuotaHistoryProbeOptions {
  /** Skip apps the user has hidden; when omitted every probe app is polled. */
  visibleApps?: Partial<VisibleApps>;
  enabled?: boolean;
}

export function useQuotaHistoryProbe({
  visibleApps,
  enabled = true,
}: UseQuotaHistoryProbeOptions = {}) {
  const queryClient = useQueryClient();

  // Fixed-length query list (enabled per app) keeps the observer set stable
  // across visibility changes instead of tearing observers down and up.
  const results = useQueries({
    queries: PROBE_APPS.map((appId) => ({
      queryKey: subscriptionKeys.quota(appId),
      queryFn: () => subscriptionApi.getQuota(appId),
      enabled: enabled && (visibleApps ? visibleApps[appId] !== false : true),
      refetchInterval: PROBE_INTERVAL_MS,
      refetchIntervalInBackground: true,
      staleTime: PROBE_INTERVAL_MS,
      retry: 1,
    })),
  });

  // `dataUpdatedAt` changes on every settled fetch, including a re-fetch that
  // returns the same numbers — the backend reports "no new information" for
  // those and we skip the invalidation.
  const settleSignature = results.map((r) => r.dataUpdatedAt).join(",");
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;

    const persist = async () => {
      let recorded = false;
      for (const [index, appId] of PROBE_APPS.entries()) {
        const sample = toQuotaSamples(results[index]?.data, Date.now());
        if (!sample) continue;
        try {
          const changed = await quotaHistoryApi.record(
            appId,
            sample.measuredAt,
            sample.tiers,
          );
          recorded = recorded || changed;
        } catch (error) {
          // A failed write costs one hourly sample; never break the app over it.
          console.warn("[quotaHistoryProbe] failed to record quota", error);
        }
      }
      if (recorded && !cancelled) {
        queryClient.invalidateQueries({ queryKey: quotaHistoryKeys.all });
      }
    };

    void persist();
    return () => {
      cancelled = true;
    };
    // settleSignature stands in for the results array, which is a new object
    // on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, settleSignature]);
}
