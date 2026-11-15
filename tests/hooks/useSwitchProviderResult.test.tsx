import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import { useSwitchProviderMutation } from "@/lib/query/mutations";
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
});
