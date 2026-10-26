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
});
