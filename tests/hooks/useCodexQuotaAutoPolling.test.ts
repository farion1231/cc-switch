import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider, QueryClient } from "@tanstack/react-query";
import { createElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { mergeNewerCodexQuota } from "@/hooks/useCodexQuotaAutoPolling";
import { useCodexQuotaAutoPolling } from "@/hooks/useCodexQuotaAutoPolling";
import { CODEX_QUOTA_ACTIVATION_STATES_QUERY_KEY } from "@/hooks/useCodexQuotaAutoPolling";
import { subscriptionApi } from "@/lib/api";
import { useCodexOauthQuotaByAccountId } from "@/lib/query/subscription";
import type { SubscriptionQuota } from "@/types/subscription";

function quota(queriedAt: number, utilization: number): SubscriptionQuota {
  return {
    tool: "codex_oauth",
    credentialStatus: "valid",
    credentialMessage: null,
    success: true,
    tiers: [
      {
        name: "five_hour",
        utilization,
        resetsAt: null,
      },
    ],
    extraUsage: null,
    error: null,
    queriedAt,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

describe("Codex quota automatic cache ordering", () => {
  it("keeps a newer manual observation when an older automatic response arrives", () => {
    const newer = quota(20, 12);
    const older = quota(0, 80);
    expect(mergeNewerCodexQuota(newer, older)).toBe(newer);
  });

  it("accepts a newer observation and an error timestamp without inventing freshness", () => {
    const older = quota(0, 80);
    const newer = quota(60, 0);
    expect(mergeNewerCodexQuota(older, newer)).toBe(newer);
    expect(mergeNewerCodexQuota(undefined, newer)).toBe(newer);
    expect(mergeNewerCodexQuota(newer, { ...older, queriedAt: null })).toBe(
      newer,
    );
  });

  it("keeps a preflight failure visible after the production hook invalidates and refetches state", async () => {
    vi.useFakeTimers();
    const previousTauri = (
      window as unknown as { __TAURI_INTERNALS__?: unknown }
    ).__TAURI_INTERNALS__;
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    const state = {
      accountId: "poll-account",
      enabled: true,
      effectiveIntervalMinutes: 1,
      authorizationRevision: 7,
      state: { kind: "eligible" },
      updatedAt: 1,
    };
    const list = vi
      .spyOn(subscriptionApi, "listCodexQuotaActivationStates")
      .mockResolvedValue([state]);
    const poll = vi
      .spyOn(subscriptionApi, "pollCodexOauthQuotaForActivation")
      .mockResolvedValue({
        accountId: state.accountId,
        polled: true,
        attempted: false,
        target: "five_hour",
        status: "model_unavailable",
        reason: "gpt-6-luna_not_in_catalog",
        quota: null,
      });
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client }, children);
    renderHook(() => useCodexQuotaAutoPolling(), { wrapper });
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(list).toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
      await Promise.resolve();
      await Promise.resolve();
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(poll).toHaveBeenCalledTimes(1);
    const current = client.getQueryData<Array<Record<string, unknown>>>(
      CODEX_QUOTA_ACTIVATION_STATES_QUERY_KEY,
    );
    expect(current?.[0]?.lastPollStatus).toBe("model_unavailable");
    expect(current?.[0]?.lastPollReason).toBe("gpt-6-luna_not_in_catalog");

    list.mockRestore();
    poll.mockRestore();
    vi.useRealTimers();
    if (previousTauri === undefined) {
      Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    } else {
      Object.defineProperty(window, "__TAURI_INTERNALS__", {
        configurable: true,
        value: previousTauri,
      });
    }
  });

  it("does not publish a superseded automatic quota while the frontend keeps an old revision", async () => {
    vi.useFakeTimers();
    const previousTauri = (
      window as unknown as { __TAURI_INTERNALS__?: unknown }
    ).__TAURI_INTERNALS__;
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    const state = {
      accountId: "stale-poll-account",
      enabled: true,
      effectiveIntervalMinutes: 1,
      authorizationRevision: 7,
      state: { kind: "eligible" },
      updatedAt: 1,
    };
    const list = vi
      .spyOn(subscriptionApi, "listCodexQuotaActivationStates")
      .mockResolvedValue([state]);
    const poll = vi
      .spyOn(subscriptionApi, "pollCodexOauthQuotaForActivation")
      .mockResolvedValue({
        accountId: state.accountId,
        polled: true,
        attempted: false,
        target: null,
        status: "superseded",
        reason: "automatic quota result superseded by account invalidation",
        quota: { ...quota(10, 90), lifecycle: "superseded" },
      });
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const key = ["codex_oauth", "quota", state.accountId] as const;
    client.setQueryData(key, quota(20, 20));
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client }, children);
    renderHook(() => useCodexQuotaAutoPolling(), { wrapper });
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
      await Promise.resolve();
      await Promise.resolve();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(poll).toHaveBeenCalledTimes(1);
    expect(client.getQueryData<SubscriptionQuota>(key)?.queriedAt).toBe(20);
    const current = client.getQueryData<Array<Record<string, unknown>>>(
      CODEX_QUOTA_ACTIVATION_STATES_QUERY_KEY,
    );
    expect(current?.[0]?.lastPollStatus).toBe("superseded");

    list.mockRestore();
    poll.mockRestore();
    vi.useRealTimers();
    if (previousTauri === undefined) {
      Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    } else {
      Object.defineProperty(window, "__TAURI_INTERNALS__", {
        configurable: true,
        value: previousTauri,
      });
    }
  });

  it("orders manual and automatic completions through the real QueryClient queryFn", async () => {
    const manualOlder = deferred<SubscriptionQuota>();
    const automaticNewer = deferred<SubscriptionQuota>();
    const getQuota = vi
      .spyOn(subscriptionApi, "getCodexOauthQuota")
      .mockReturnValueOnce(manualOlder.promise)
      .mockReturnValueOnce(automaticNewer.promise);
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client }, children);
    const older = quota(10, 80);
    const newer = quota(20, 0);
    renderHook(
      () => useCodexOauthQuotaByAccountId("manual-older", { enabled: true }),
      { wrapper },
    );
    const key = ["codex_oauth", "quota", "manual-older"] as const;
    automaticNewer.resolve(newer);
    await act(async () => {
      await client.setQueryData(key, (current: SubscriptionQuota | undefined) =>
        mergeNewerCodexQuota(current, newer),
      );
      manualOlder.resolve(older);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(client.getQueryData<SubscriptionQuota>(key)).toBe(newer);

    const automaticOlder = deferred<SubscriptionQuota>();
    const manualNewer = deferred<SubscriptionQuota>();
    getQuota
      .mockReset()
      .mockReturnValueOnce(automaticOlder.promise)
      .mockReturnValueOnce(manualNewer.promise);
    const secondClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const secondWrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: secondClient }, children);
    renderHook(
      () => useCodexOauthQuotaByAccountId("automatic-older", { enabled: true }),
      { wrapper: secondWrapper },
    );
    const secondKey = ["codex_oauth", "quota", "automatic-older"] as const;
    manualNewer.resolve(newer);
    await act(async () => {
      secondClient.setQueryData(secondKey, newer);
      automaticOlder.resolve(older);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(secondClient.getQueryData<SubscriptionQuota>(secondKey)).toBe(newer);
    getQuota.mockRestore();
  });
});
