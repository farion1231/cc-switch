import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useApplyProfileMutation } from "@/lib/query/profiles";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import { createTestQueryClient } from "../utils/testQueryClient";

const applyProfileMock = vi.fn();
const updateTrayMenuMock = vi.fn();
const toastWarningMock = vi.fn();
const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();

vi.mock("@/lib/api", () => ({
  profilesApi: {
    apply: (...args: unknown[]) => applyProfileMock(...args),
  },
  providersApi: {
    updateTrayMenu: (...args: unknown[]) => updateTrayMenuMock(...args),
  },
}));

vi.mock("sonner", () => ({
  toast: {
    warning: (...args: unknown[]) => toastWarningMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

const resources = { en, ja, zh, "zh-TW": zhTW };
const officialAuthFallback =
  "无法安全清理 Codex 接管配置：清理会让保留的地址回退使用官方认证。请先恢复原配置，或为该地址配置独立认证。";
const unverifiedProfiles =
  "无法安全清理 Codex 接管配置：存在 profile 或无法检查配置目录，不能排除官方认证回退。请先恢复原配置并检查相关 profile。";
const warningPrefix =
  "[codex] auto-disable proxy takeover before profile switch failed: ";

function createWrapper() {
  const queryClient = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { wrapper, queryClient };
}

describe("useApplyProfileMutation", () => {
  beforeEach(async () => {
    applyProfileMock.mockReset();
    updateTrayMenuMock.mockReset().mockResolvedValue(undefined);
    toastWarningMock.mockReset();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    for (const [language, translation] of Object.entries(resources)) {
      i18n.addResourceBundle(language, "translation", translation, true, true);
    }
    await i18n.changeLanguage("zh");
  });

  afterEach(async () => {
    await i18n.changeLanguage("zh");
  });

  it.each(["en", "ja", "zh", "zh-TW"] as const)(
    "localizes cleanup warnings in %s without losing their context or other warnings",
    async (language) => {
      const untouchedWarning =
        "[claude] provider 'deleted-provider' no longer exists, skipped";
      const warnings = [
        untouchedWarning,
        warningPrefix + officialAuthFallback,
        warningPrefix + unverifiedProfiles,
      ];
      applyProfileMock.mockResolvedValue(warnings);
      const { wrapper, queryClient } = createWrapper();
      const { result, unmount } = renderHook(() => useApplyProfileMutation(), {
        wrapper,
      });

      // Select the language after mounting, so the warning path must use the
      // user's current choice rather than a startup-language snapshot.
      await act(async () => {
        await i18n.changeLanguage(language);
      });
      await act(async () => {
        await result.current.mutateAsync({ id: "work", scope: "codex" });
      });

      const translated = resources[language].proxy.takeover.cleanup;
      const details = [
        untouchedWarning,
        warningPrefix + translated.officialAuthFallback,
        warningPrefix + translated.unverifiedProfiles,
      ].join("\n");
      expect(applyProfileMock).toHaveBeenCalledWith("work", "codex");
      expect(updateTrayMenuMock).toHaveBeenCalledTimes(1);
      expect(toastWarningMock).toHaveBeenCalledTimes(1);
      expect(toastWarningMock).toHaveBeenCalledWith(
        i18n.t("profiles.applyWarnings", { warningCount: 3, details }),
        { closeButton: true, duration: 10000 },
      );
      expect(toastSuccessMock).not.toHaveBeenCalled();
      expect(toastErrorMock).not.toHaveBeenCalled();
      expect(warnings).toEqual([
        untouchedWarning,
        warningPrefix + officialAuthFallback,
        warningPrefix + unverifiedProfiles,
      ]);

      unmount();
      queryClient.clear();
    },
  );

  it("keeps the successful no-warning flow unchanged", async () => {
    applyProfileMock.mockResolvedValue([]);
    await i18n.changeLanguage("en");
    const { wrapper, queryClient } = createWrapper();
    const { result, unmount } = renderHook(() => useApplyProfileMutation(), {
      wrapper,
    });

    await act(async () => {
      await result.current.mutateAsync({ id: "work", scope: "codex" });
    });

    expect(toastSuccessMock).toHaveBeenCalledTimes(1);
    expect(toastSuccessMock).toHaveBeenCalledWith(
      resources.en.profiles.applySuccess,
      { closeButton: true },
    );
    expect(toastWarningMock).not.toHaveBeenCalled();
    expect(toastErrorMock).not.toHaveBeenCalled();
    unmount();
    queryClient.clear();
  });
});
