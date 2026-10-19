import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import { useSwitchProviderMutation } from "@/lib/query/mutations";
import { toast } from "@/lib/toast";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/lib/toast", () => ({ toast: { error: vi.fn() } }));

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

  it.each([
    ["en", en, ""],
    ["zh", zh, ""],
    ["zh-TW", zhTW, ""],
    ["ja", ja, ""],
    ["en", en, "切换路由失败: "],
    ["zh", zh, "切换路由失败: "],
    ["zh-TW", zhTW, "切换路由失败: "],
    ["ja", ja, "切换路由失败: "],
  ] as const)(
    "explains an unavailable official login in %s without refreshing the active card",
    async (language, resources, prefix) => {
      // The transport code, not the fallback wording, selects the translation.
      const message =
        prefix +
        JSON.stringify({
          code: "codex.official_login_unavailable",
          message:
            "The fallback wording can change independently of the toast.",
        });
      server.use(
        http.post("http://tauri.local/switch_provider", () =>
          HttpResponse.text(message, { status: 409 }),
        ),
      );
      i18n.addResourceBundle(language, "translation", resources, true, true);
      await i18n.changeLanguage(language);
      const client = createTestQueryClient();
      const invalidate = vi.spyOn(client, "invalidateQueries");
      const wrapper = ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      );
      const { result, unmount } = renderHook(
        () => useSwitchProviderMutation("codex"),
        {
          wrapper,
        },
      );
      await act(async () => {
        await expect(result.current.mutateAsync("provider-b")).rejects.toThrow(
          message,
        );
      });
      expect(toast.error).toHaveBeenCalledWith(
        resources.notifications.switchFailedTitle,
        expect.objectContaining({
          description: resources.notifications.switchFailed.replace(
            "{{error}}",
            resources.notifications.codexOfficialLoginUnavailable,
          ),
        }),
      );
      expect(invalidate).not.toHaveBeenCalled();
      expect(resources.notifications.codexOfficialLoginUnavailable).toContain(
        resources.nav.auth,
      );
      unmount();
      client.clear();
      await i18n.changeLanguage("zh");
    },
  );

  it("keeps unstructured errors unchanged instead of matching English wording", async () => {
    const message =
      "Codex official login is unavailable: an unstructured error";
    server.use(
      http.post("http://tauri.local/switch_provider", () =>
        HttpResponse.text(message, { status: 409 }),
      ),
    );
    await i18n.changeLanguage("en");
    const client = createTestQueryClient();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result, unmount } = renderHook(
      () => useSwitchProviderMutation("codex"),
      { wrapper },
    );
    await act(async () => {
      await expect(result.current.mutateAsync("provider-b")).rejects.toThrow(
        message,
      );
    });
    expect(toast.error).toHaveBeenCalledWith(
      en.notifications.switchFailedTitle,
      expect.objectContaining({
        description: en.notifications.switchFailed.replace(
          "{{error}}",
          message,
        ),
      }),
    );
    unmount();
    client.clear();
    await i18n.changeLanguage("zh");
  });
});
