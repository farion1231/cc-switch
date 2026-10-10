import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { useUsageCacheBridge } from "@/hooks/useUsageCacheBridge";
import { KEEP_LAST_GOOD_MS } from "@/lib/query/queries";
import type { SubscriptionQuota } from "@/types/subscription";

const events = vi.hoisted(() => ({
  callback: null as ((payload: unknown) => void) | null,
}));
vi.mock("@/hooks/useTauriEvent", () => ({
  useTauriEvent: (name: string, callback: (payload: unknown) => void) => {
    if (name === "usage-cache-updated") events.callback = callback;
  },
}));
const quota: SubscriptionQuota = {
  tool: "codex",
  credentialStatus: "valid",
  credentialMessage: null,
  success: true,
  tiers: [],
  extraUsage: null,
  error: null,
  queriedAt: 1,
};

describe("useUsageCacheBridge", () => {
  it("writes Codex events to shared account/card caches and preserves other accounts", () => {
    const client = new QueryClient();
    client.setQueryData(["codex_oauth", "quota", "other"], quota);
    renderHook(() => useUsageCacheBridge(), {
      wrapper: ({ children }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    });
    const failure = { ...quota, success: false, error: "timeout" };
    act(() =>
      events.callback?.({
        kind: "codex_oauth",
        accountId: "alice",
        data: failure,
      }),
    );
    expect(client.getQueryData(["codex_oauth", "quota", "alice"])).toEqual(
      failure,
    );
    expect(client.getQueryData(["codex_oauth", "quota", "other"])).toEqual(
      quota,
    );
    expect(
      client.getQueryData(["subscription", "quota", "codex"]),
    ).toBeUndefined();
  });
  it("preserves unmounted account success and its age on transient failure", () => {
    const client = new QueryClient();
    const key = ["codex_oauth", "quota", "alice"];
    const at = Date.now() - 60_000;
    client.setQueryData(key, quota, { updatedAt: at });
    renderHook(() => useUsageCacheBridge(), {
      wrapper: ({ children }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    });
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const failure = {
      ...quota,
      success: false,
      error: "Network error: timeout",
    };
    act(() =>
      events.callback?.({
        kind: "codex_oauth",
        accountId: "alice",
        data: failure,
      }),
    );
    expect(client.getQueryData(key)).toEqual(quota);
    expect(client.getQueryState(key)?.dataUpdatedAt).toBe(at);
    expect(client.getQueryState(key)?.isInvalidated).toBe(true);
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: key,
      exact: true,
      refetchType: "none",
    });
  });

  it("publishes a transient failure after the original success window expires", () => {
    const client = new QueryClient();
    const key = ["codex_oauth", "quota", "alice"];
    client.setQueryData(key, quota, {
      updatedAt: Date.now() - KEEP_LAST_GOOD_MS - 1,
    });
    renderHook(() => useUsageCacheBridge(), {
      wrapper: ({ children }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    });
    const failure = {
      ...quota,
      success: false,
      error: "Network error: timeout",
    };
    act(() =>
      events.callback?.({
        kind: "codex_oauth",
        accountId: "alice",
        data: failure,
      }),
    );
    expect(client.getQueryData(key)).toEqual(failure);
  });

  it("immediately replaces cached success for expired credentials", () => {
    const client = new QueryClient();
    const key = ["codex_oauth", "quota", "alice"];
    client.setQueryData(key, quota);
    renderHook(() => useUsageCacheBridge(), {
      wrapper: ({ children }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    });
    const failure = {
      ...quota,
      success: false,
      credentialStatus: "expired",
      error: "HTTP 401",
    };
    act(() =>
      events.callback?.({
        kind: "codex_oauth",
        accountId: "alice",
        data: failure,
      }),
    );
    expect(client.getQueryData(key)).toEqual(failure);
  });
});
