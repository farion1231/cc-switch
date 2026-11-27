import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import { useSwitchProviderMutation } from "@/lib/query/mutations";
import { useSubscriptionQuota } from "@/lib/query/subscription";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

// Exercise the actual invoke -> mutation -> caller path: the result must not be
// swallowed by onSuccess/cache invalidation before notification selection.
describe("provider switch result transport", () => {
  it.each(["route", "stack", "remembered"] as const)(
    "preserves the applied %s result and warnings",
    async (kind) => {
      const expected = {
        warnings: ["codex_auth_cleanup_failed: test"],
        route: { kind, clientConfigChanged: kind === "stack" },
      };
      server.use(
        http.post("http://tauri.local/switch_provider", () =>
          HttpResponse.json(expected),
        ),
      );
      const client = createTestQueryClient();
      const wrapper = ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      );
      const { result } = renderHook(() => useSwitchProviderMutation("codex"), {
        wrapper,
      });
      let actual;
      await act(async () => {
        actual = await result.current.mutateAsync("provider-b");
      });
      expect(actual).toEqual(expected);
      client.clear();
    },
  );

  // 切到绑号的官方卡时后端可能把默认登录收成新账号，授权中心的 Claude 列表要重读
  it.each([
    ["claude", true],
    ["codex", false],
  ] as const)(
    "invalidates the Claude account list after a %s switch: %s",
    async (appId, invalidated) => {
      server.use(
        http.post("http://tauri.local/switch_provider", () =>
          HttpResponse.json({ warnings: [] }),
        ),
      );
      const client = createTestQueryClient();
      const statusKey = ["managed-auth-status", "claude_official"];
      client.setQueryData(statusKey, { accounts: [] });
      const wrapper = ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      );
      const { result } = renderHook(() => useSwitchProviderMutation(appId), {
        wrapper,
      });
      await act(async () => {
        await result.current.mutateAsync("provider-b");
      });
      expect(client.getQueryState(statusKey)?.isInvalidated).toBe(invalidated);
      client.clear();
    },
  );

  // 切到绑号的 Grok 官方卡会把原 CLI 登录收进授权中心再换号；切换失败前也可能已经收过
  it.each([
    ["grokbuild", true, false],
    ["grokbuild", true, true],
    ["claude", false, false],
  ] as const)(
    "invalidates the xAI account list and Grok CLI quota after a %s switch: %s (failed=%s)",
    async (appId, invalidated, fails) => {
      server.use(
        http.post("http://tauri.local/switch_provider", () =>
          fails
            ? HttpResponse.json("切换失败", { status: 500 })
            : HttpResponse.json({ warnings: [] }),
        ),
      );
      const client = createTestQueryClient();
      const statusKey = ["managed-auth-status", "xai_oauth"];
      const quotaKey = ["subscription", "quota", "grokbuild"];
      client.setQueryData(statusKey, { accounts: [] });
      client.setQueryData(quotaKey, { success: true, tiers: [] });
      const wrapper = ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      );
      const { result } = renderHook(() => useSwitchProviderMutation(appId), {
        wrapper,
      });
      await act(async () => {
        await result.current.mutateAsync("provider-b").catch(() => undefined);
      });
      expect(client.getQueryState(statusKey)?.isInvalidated).toBe(invalidated);
      expect(client.getQueryState(quotaKey)?.isInvalidated).toBe(invalidated);
      client.clear();
    },
  );

  it("does not block Claude provider switch while an active quota query is hanging", async () => {
    let quotaCalls = 0;
    let trayCalls = 0;
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
      http.post("http://tauri.local/switch_provider", () =>
        HttpResponse.json({ warnings: [] }),
      ),
      http.post("http://tauri.local/update_tray_menu", () => {
        trayCalls += 1;
        return HttpResponse.json(true);
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
        switchMutation: useSwitchProviderMutation("claude"),
      }),
      { wrapper },
    );

    await waitFor(() => expect(result.current.quota.isSuccess).toBe(true));

    await act(async () => {
      await result.current.switchMutation.mutateAsync("provider-b");
    });

    expect(trayCalls).toBe(1);
    expect(quotaCalls).toBe(2);
    expect(client.getQueryState(statusKey)?.isInvalidated).toBe(true);
    expect(client.getQueryState(defaultPlanKey)?.isInvalidated).toBe(true);
    expect(client.getQueryState(quotaKey)?.isInvalidated).toBe(true);
    client.clear();
  });
});
