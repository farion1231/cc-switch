import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useModeActions } from "@/hooks/useModeActions";
import { useSubscriptionQuota } from "@/lib/query/subscription";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
    error: vi.fn(),
  },
}));

describe("useModeActions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("invalidates Claude official login caches on exitToDirect without blocking on quota refetch", async () => {
    const takeoverCalls: unknown[] = [];
    const switchCalls: unknown[] = [];
    let quotaCalls = 0;

    server.use(
      http.post("http://tauri.local/get_settings", () =>
        HttpResponse.json({ officialQuotaManualOnly: false }),
      ),
      http.post("http://tauri.local/get_subscription_quota", () => {
        quotaCalls += 1;
        if (quotaCalls === 1) {
          return HttpResponse.json({
            tool: "claude",
            credentialStatus: "valid",
            credentialMessage: null,
            success: true,
            tiers: [],
            extraUsage: null,
            error: null,
            queriedAt: 1,
          });
        }
        return new Promise(() => {});
      }),
      http.post(
        "http://tauri.local/set_proxy_takeover_for_app",
        async ({ request }) => {
          takeoverCalls.push(await request.json());
          return HttpResponse.json(null);
        },
      ),
      http.post("http://tauri.local/switch_provider", async ({ request }) => {
        switchCalls.push(await request.json());
        return HttpResponse.json({ warnings: [] });
      }),
    );

    const client = createTestQueryClient();
    const statusKey = ["managed-auth-status", "claude_official"];
    const defaultPlanKey = ["claude_official", "default_plan"];
    const quotaKey = ["subscription", "quota", "claude"];
    client.setQueryData(statusKey, { accounts: [] });
    client.setQueryData(defaultPlanKey, { subscriptionType: "pro" });

    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(
      () => ({
        quota: useSubscriptionQuota("claude", true),
        actions: useModeActions("claude"),
      }),
      { wrapper },
    );

    await waitFor(() => expect(result.current.quota.isSuccess).toBe(true));

    await act(async () => {
      await result.current.actions.exitToDirect({
        directProviderId: "p1",
        useProviderId: "p2",
        providerName: "Provider 2",
      });
    });

    expect(takeoverCalls).toEqual([
      { appType: "claude", enabled: false, stack: false, route: null },
    ]);
    expect(switchCalls).toEqual([{ id: "p2", app: "claude" }]);
    expect(quotaCalls).toBe(2);
    expect(client.getQueryState(statusKey)?.isInvalidated).toBe(true);
    expect(client.getQueryState(defaultPlanKey)?.isInvalidated).toBe(true);
    expect(client.getQueryState(quotaKey)?.isInvalidated).toBe(true);
    client.clear();
  });

  it("does not invalidate Claude official caches on exitToDirect for other apps", async () => {
    server.use(
      http.post("http://tauri.local/set_proxy_takeover_for_app", () =>
        HttpResponse.json(null),
      ),
      http.post("http://tauri.local/switch_provider", () =>
        HttpResponse.json({ warnings: [] }),
      ),
    );

    const client = createTestQueryClient();
    const statusKey = ["managed-auth-status", "claude_official"];
    const defaultPlanKey = ["claude_official", "default_plan"];
    const quotaKey = ["subscription", "quota", "claude"];
    client.setQueryData(statusKey, { accounts: [] });
    client.setQueryData(defaultPlanKey, { subscriptionType: "pro" });
    client.setQueryData(quotaKey, { success: true, tiers: [] });

    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useModeActions("codex"), { wrapper });

    await act(async () => {
      await result.current.exitToDirect({
        directProviderId: "p1",
        useProviderId: "p2",
        providerName: "Provider 2",
      });
    });

    expect(client.getQueryState(statusKey)?.isInvalidated).toBe(false);
    expect(client.getQueryState(defaultPlanKey)?.isInvalidated).toBe(false);
    expect(client.getQueryState(quotaKey)?.isInvalidated).toBe(false);
    client.clear();
  });
});
