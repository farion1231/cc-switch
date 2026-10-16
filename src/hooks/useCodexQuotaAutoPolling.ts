import { useEffect, useRef } from "react";
import {
  QueryClientContext,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { useContext } from "react";
import { subscriptionApi } from "@/lib/api";
import type { CodexQuotaActivationState } from "@/lib/api";
import {
  codexQuotaActivationStatesKey,
  mergeNewerCodexOauthQuota,
  publishCodexOauthQuota,
} from "@/lib/query/subscription";

type LatestPollSummary = {
  authorizationRevision: number;
  polledAtMs: number;
  status: string;
  reason: string | null;
};

const latestPollSummaries = new Map<string, LatestPollSummary>();

function mergeLatestPollSummary(
  state: CodexQuotaActivationState,
): CodexQuotaActivationState {
  const summary = latestPollSummaries.get(state.accountId);
  if (
    !summary ||
    summary.authorizationRevision !== state.authorizationRevision
  ) {
    if (summary) latestPollSummaries.delete(state.accountId);
    return state;
  }
  return {
    ...state,
    lastPolledAtMs: summary.polledAtMs,
    lastPollStatus: summary.status,
    lastPollReason: summary.reason,
  };
}

export const CODEX_QUOTA_ACTIVATION_STATES_QUERY_KEY =
  codexQuotaActivationStatesKey;

export const mergeNewerCodexQuota = mergeNewerCodexOauthQuota;

export function useCodexQuotaActivationStates(
  options: { polling?: boolean } = {},
) {
  const polling = options.polling ?? false;
  const queryClient = useContext(QueryClientContext);
  if (!queryClient) {
    return {
      data: undefined,
      isError: false,
      isPending: false,
    } as const;
  }
  return useQuery<CodexQuotaActivationState[]>({
    queryKey: CODEX_QUOTA_ACTIVATION_STATES_QUERY_KEY,
    queryFn: async () => {
      const states = await subscriptionApi.listCodexQuotaActivationStates();
      const accountIds = new Set(states.map((state) => state.accountId));
      for (const accountId of latestPollSummaries.keys()) {
        if (!accountIds.has(accountId)) latestPollSummaries.delete(accountId);
      }
      return states.map(mergeLatestPollSummary);
    },
    refetchInterval: polling ? 60_000 : false,
    refetchIntervalInBackground: polling,
    staleTime: 30_000,
    enabled:
      typeof window !== "undefined" &&
      "__TAURI_INTERNALS__" in (window as unknown as Record<string, unknown>),
  });
}

/**
 * Root lifetime account timers for the opt-in activation entry. Timers are
 * created per account and only call the dedicated automatic IPC; ordinary
 * quota query observers never reach this hook.
 */
export function useCodexQuotaAutoPolling() {
  const queryClient = useQueryClient();
  const timers = useRef(
    new Map<
      string,
      { interval: number; timer: number; authorizationRevision: number }
    >(),
  );
  const { data: states = [] } = useCodexQuotaActivationStates({
    polling: true,
  });

  useEffect(() => {
    const desired = new Map<
      string,
      { interval: number; authorizationRevision: number }
    >();
    for (const state of states) {
      if (!state.enabled) continue;
      const interval = state.effectiveIntervalMinutes ?? 0;
      if (interval <= 0) continue;
      desired.set(state.accountId, {
        interval,
        authorizationRevision: state.authorizationRevision,
      });
    }

    for (const [accountId, entry] of timers.current) {
      const next = desired.get(accountId);
      if (
        next?.interval === entry.interval &&
        next.authorizationRevision === entry.authorizationRevision
      )
        continue;
      window.clearInterval(entry.timer);
      timers.current.delete(accountId);
    }
    for (const [accountId, desiredEntry] of desired) {
      const { interval, authorizationRevision } = desiredEntry;
      if (timers.current.has(accountId)) continue;
      const timer = window.setInterval(() => {
        void subscriptionApi
          .pollCodexOauthQuotaForActivation(accountId)
          .then((result) => {
            const current = queryClient.getQueryData<
              CodexQuotaActivationState[]
            >(CODEX_QUOTA_ACTIVATION_STATES_QUERY_KEY);
            const currentState = current?.find(
              (state) => state.accountId === result.accountId,
            );
            if (
              !currentState ||
              currentState.authorizationRevision !== authorizationRevision
            ) {
              latestPollSummaries.delete(result.accountId);
              return;
            }
            // The Rust quota DTO carries the request-start timestamp.  Use it
            // for a real poll summary; Date.now is only a local receipt
            // fallback for failures that never produced a quota DTO.
            const polledAtMs = result.quota?.queriedAt ?? Date.now();
            if (result.polled) {
              latestPollSummaries.set(result.accountId, {
                authorizationRevision,
                polledAtMs,
                status: result.status,
                reason: result.reason ?? null,
              });
            }
            if (result.quota) {
              const quota = result.quota;
              if (
                ![
                  "superseded",
                  "stale_result",
                  "authorization_changed",
                  "disabled",
                ].includes(result.status) &&
                quota.lifecycle !== "superseded"
              ) {
                publishCodexOauthQuota(queryClient, result.accountId, quota);
              }
            }
            queryClient.setQueryData<CodexQuotaActivationState[]>(
              CODEX_QUOTA_ACTIVATION_STATES_QUERY_KEY,
              (current) =>
                current?.map((state) => {
                  if (state.accountId !== result.accountId) return state;
                  const next = mergeLatestPollSummary(state);
                  return result.attempted
                    ? {
                        ...next,
                        lastAttemptAtMs: polledAtMs,
                        lastAttemptStatus: result.status,
                        lastReason: result.reason ?? null,
                      }
                    : next;
                }),
            );
            void queryClient.invalidateQueries({
              queryKey: CODEX_QUOTA_ACTIVATION_STATES_QUERY_KEY,
            });
          })
          .catch((error: unknown) => {
            const reason =
              error instanceof Error
                ? error.message
                : "automatic_quota_poll_failed";
            queryClient.setQueryData<CodexQuotaActivationState[]>(
              CODEX_QUOTA_ACTIVATION_STATES_QUERY_KEY,
              (current) =>
                current?.map((state) => {
                  if (state.accountId !== accountId) return state;
                  const summary = {
                    authorizationRevision,
                    polledAtMs: Date.now(),
                    status: "poll_failed",
                    reason: reason.slice(0, 256),
                  } satisfies LatestPollSummary;
                  latestPollSummaries.set(accountId, summary);
                  return mergeLatestPollSummary(state);
                }),
            );
          });
      }, interval * 60_000);
      timers.current.set(accountId, {
        interval,
        timer,
        authorizationRevision,
      });
    }
    return () => {
      // Keep timers across state refetches; they are cleared on unmount below.
    };
  }, [queryClient, states]);

  useEffect(
    () => () => {
      for (const entry of timers.current.values())
        window.clearInterval(entry.timer);
      timers.current.clear();
    },
    [],
  );
}
