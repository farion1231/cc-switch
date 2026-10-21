import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useManagedAuth } from "@/components/providers/forms/hooks/useManagedAuth";
import { CODEX_OAUTH_DUPLICATE_ACCOUNT_ERROR } from "@/lib/api/auth";

const apiMocks = vi.hoisted(() => ({
  authGetStatus: vi.fn(),
  authStartLogin: vi.fn(),
  authPollForAccount: vi.fn(),
  authCancelLogin: vi.fn(),
  authRemoveAccount: vi.fn(),
  authLogout: vi.fn(),
}));
const toastMocks = vi.hoisted(() => ({
  success: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  authApi: {
    authGetStatus: (...args: unknown[]) => apiMocks.authGetStatus(...args),
    authStartLogin: (...args: unknown[]) => apiMocks.authStartLogin(...args),
    authPollForAccount: (...args: unknown[]) =>
      apiMocks.authPollForAccount(...args),
    authCancelLogin: (...args: unknown[]) => apiMocks.authCancelLogin(...args),
    authLogout: (...args: unknown[]) => apiMocks.authLogout(...args),
    authRemoveAccount: (...args: unknown[]) =>
      apiMocks.authRemoveAccount(...args),
  },
  settingsApi: {
    openExternal: vi.fn().mockResolvedValue(undefined),
  },
}));

vi.mock("@/lib/clipboard", () => ({
  copyText: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("sonner", () => ({
  toast: {
    success: toastMocks.success,
  },
}));

function createWrapper(
  queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  }),
) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("useManagedAuth", () => {
  it("opens browser login without copying a device code and keeps the reauth target", async () => {
    const { copyText } = await import("@/lib/clipboard");
    const { settingsApi } = await import("@/lib/api");
    vi.mocked(copyText).mockClear();
    vi.mocked(settingsApi.openExternal).mockClear();
    apiMocks.authStartLogin.mockResolvedValue({
      provider: "codex_oauth",
      device_code: "browser:flow",
      user_code: "",
      verification_uri: "https://auth.openai.com/oauth/authorize?state=random",
      expires_in: 900,
      interval: 1,
    });
    const { result, unmount } = renderHook(
      () => useManagedAuth("codex_oauth", undefined, "browser"),
      { wrapper: createWrapper() },
    );
    await waitFor(() => expect(result.current.isStatusSuccess).toBe(true));
    act(() => result.current.reauthAccount("acct-1"));
    await waitFor(() => expect(result.current.isPolling).toBe(true));
    expect(apiMocks.authStartLogin).toHaveBeenCalledWith(
      "codex_oauth",
      undefined,
      "acct-1",
      "browser",
    );
    await waitFor(() =>
      expect(settingsApi.openExternal).toHaveBeenCalledWith(
        "https://auth.openai.com/oauth/authorize?state=random",
      ),
    );
    expect(copyText).not.toHaveBeenCalled();
    act(() => result.current.cancelAuth());
    await waitFor(() =>
      expect(apiMocks.authCancelLogin).toHaveBeenCalledWith(
        "codex_oauth",
        "browser:flow",
      ),
    );
    unmount();
  });

  beforeEach(() => {
    apiMocks.authGetStatus.mockReset().mockResolvedValue({
      provider: "codex_oauth",
      authenticated: true,
      default_account_id: "acct-1",
      accounts: [
        {
          id: "acct-1",
          provider: "codex_oauth",
          login: "user@example.com",
          avatar_url: null,
          authenticated_at: 1,
          is_default: true,
          github_domain: "",
          reauth_required: false,
          requires_reauth: false,
        },
      ],
    });
    apiMocks.authStartLogin
      .mockReset()
      .mockImplementation(() => new Promise(() => {}));
    apiMocks.authPollForAccount.mockReset().mockResolvedValue(null);
    apiMocks.authCancelLogin.mockReset().mockResolvedValue(true);
    apiMocks.authRemoveAccount.mockReset().mockResolvedValue(undefined);
    apiMocks.authLogout.mockReset().mockResolvedValue(undefined);
  });

  describe.each(["codex_oauth", "agy_oauth"] as const)(
    "%s browser login",
    (provider) => {
      const browserResponse = {
        provider,
        device_code: "browser:flow",
        user_code: "",
        verification_uri: "https://example.com/authorize",
        expires_in: 60,
        interval: 1,
      };
      const renderBrowserAuth = () =>
        renderHook(
          () =>
            useManagedAuth(
              provider,
              undefined,
              provider === "codex_oauth" ? "browser" : undefined,
            ),
          { wrapper: createWrapper() },
        );

      it("polls every 1500 ms without overlapping and resumes after pending errors", async () => {
        const { copyText } = await import("@/lib/clipboard");
        const { settingsApi } = await import("@/lib/api");
        let resolvePoll!: (account: null) => void;
        apiMocks.authStartLogin.mockResolvedValue(browserResponse);
        apiMocks.authPollForAccount
          .mockImplementationOnce(
            () => new Promise((resolve) => (resolvePoll = resolve)),
          )
          .mockRejectedValueOnce(new Error("authorization_pending"))
          .mockRejectedValueOnce(new Error("slow_down"))
          .mockResolvedValueOnce({ id: "acct-1" });
        const { result, unmount } = renderBrowserAuth();
        await waitFor(() => expect(result.current.isStatusSuccess).toBe(true));
        vi.useFakeTimers();
        try {
          await act(async () => result.current.startAuth());
          expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(1);
          expect(settingsApi.openExternal).toHaveBeenCalledWith(
            browserResponse.verification_uri,
          );
          expect(copyText).not.toHaveBeenCalled();
          await act(() => vi.advanceTimersByTimeAsync(4500));
          expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(1);

          await act(async () => resolvePoll(null));
          await act(() => vi.advanceTimersByTimeAsync(1499));
          expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(1);
          for (const elapsed of [1, 1500, 1500]) {
            await act(() => vi.advanceTimersByTimeAsync(elapsed));
          }
          expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(4);
          expect(result.current.pollingState).toBe("idle");
          expect(result.current.deviceCode).toBeNull();
          await act(() => vi.advanceTimersByTimeAsync(60000));
          expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(4);
          expect(apiMocks.authCancelLogin).not.toHaveBeenCalled();
        } finally {
          unmount();
          vi.useRealTimers();
        }
      });

      it.each(["cancel", "expire", "replace", "unmount"] as const)(
        "ignores an in-flight result after %s and cancels the old flow",
        async (ending) => {
          let resolvePoll!: (account: object) => void;
          apiMocks.authStartLogin
            .mockResolvedValueOnce(browserResponse)
            .mockResolvedValueOnce({
              ...browserResponse,
              device_code: "browser:replacement",
            });
          apiMocks.authPollForAccount
            .mockImplementationOnce(
              () => new Promise((resolve) => (resolvePoll = resolve)),
            )
            .mockImplementation(() => new Promise(() => {}));
          const { result, unmount } = renderBrowserAuth();
          await waitFor(() =>
            expect(result.current.isStatusSuccess).toBe(true),
          );
          vi.useFakeTimers();
          try {
            await act(async () => result.current.startAuth());
            expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(1);
            const statusCalls = apiMocks.authGetStatus.mock.calls.length;
            if (ending === "expire") {
              await act(() => vi.advanceTimersByTimeAsync(60000));
              expect(result.current.pollingState).toBe("error");
              expect(result.current.error).toBe(
                "Login expired. Please try again.",
              );
            } else if (ending === "replace") {
              await act(async () => result.current.startAuth());
              expect(result.current.deviceCode?.device_code).toBe(
                "browser:replacement",
              );
            } else if (ending === "unmount") {
              unmount();
            } else {
              await act(async () => result.current.cancelAuth());
              expect(result.current.pollingState).toBe("idle");
              expect(result.current.deviceCode).toBeNull();
            }
            expect(apiMocks.authCancelLogin).toHaveBeenCalledWith(
              provider,
              "browser:flow",
            );

            await act(async () => resolvePoll({ id: "stale-account" }));
            await act(() => vi.advanceTimersByTimeAsync(4500));
            expect(apiMocks.authGetStatus).toHaveBeenCalledTimes(statusCalls);
            expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(
              ending === "replace" ? 2 : 1,
            );
            if (ending === "replace") {
              expect(result.current.isPolling).toBe(true);
              expect(result.current.deviceCode?.device_code).toBe(
                "browser:replacement",
              );
            }
          } finally {
            unmount();
            vi.useRealTimers();
          }
        },
      );
    },
  );

  it.each([
    [1, 8000],
    [10, 13000],
  ])("keeps device-code backoff for interval %s", async (interval, delay) => {
    apiMocks.authStartLogin.mockResolvedValue({
      provider: "codex_oauth",
      device_code: "device-1",
      user_code: "ABCD-EFGH",
      verification_uri: "https://example.com/device",
      expires_in: 600,
      interval,
    });
    const { result, unmount } = renderHook(
      () => useManagedAuth("codex_oauth"),
      {
        wrapper: createWrapper(),
      },
    );
    await waitFor(() => expect(result.current.isStatusSuccess).toBe(true));
    vi.useFakeTimers();
    try {
      await act(async () => result.current.startAuth());
      expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(1);
      await act(() => vi.advanceTimersByTimeAsync(delay - 1));
      expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(1);
      await act(() => vi.advanceTimersByTimeAsync(1));
      expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(2);
    } finally {
      unmount();
      vi.useRealTimers();
    }
  });

  it("starts reauthentication for the selected account", async () => {
    const { result } = renderHook(() => useManagedAuth("codex_oauth"), {
      wrapper: createWrapper(),
    });
    await waitFor(() => expect(result.current.isStatusSuccess).toBe(true));

    act(() => result.current.reauthAccount("acct-1"));

    await waitFor(() =>
      expect(apiMocks.authStartLogin).toHaveBeenCalledWith(
        "codex_oauth",
        undefined,
        "acct-1",
      ),
    );
  });

  it("retries reauthentication for the same target account", async () => {
    apiMocks.authStartLogin.mockRejectedValue(new Error("start failed"));
    const { result } = renderHook(() => useManagedAuth("codex_oauth"), {
      wrapper: createWrapper(),
    });
    await waitFor(() => expect(result.current.isStatusSuccess).toBe(true));

    act(() => result.current.reauthAccount("acct-1"));
    await waitFor(() => expect(result.current.pollingState).toBe("error"));
    act(() => result.current.retryAuth());

    await waitFor(() =>
      expect(apiMocks.authStartLogin).toHaveBeenCalledTimes(2),
    );
    expect(apiMocks.authStartLogin).toHaveBeenNthCalledWith(
      2,
      "codex_oauth",
      undefined,
      "acct-1",
    );
  });

  it("localizes a duplicate Codex account error", async () => {
    apiMocks.authStartLogin.mockResolvedValue({
      provider: "codex_oauth",
      device_code: "device-1",
      user_code: "ABCD-EFGH",
      verification_uri: "https://example.com/device",
      expires_in: 600,
      interval: 5,
    });
    apiMocks.authPollForAccount.mockRejectedValue(
      new Error(CODEX_OAUTH_DUPLICATE_ACCOUNT_ERROR),
    );
    const { result } = renderHook(() => useManagedAuth("codex_oauth"), {
      wrapper: createWrapper(),
    });

    act(() => result.current.addAccount());

    await waitFor(() => expect(result.current.pollingState).toBe("error"));
    expect(result.current.error).toBe(
      "该 ChatGPT 账号已添加，请直接使用现有账号。",
    );
  });

  it("cancels the active Codex device flow in the backend", async () => {
    apiMocks.authStartLogin.mockResolvedValue({
      provider: "codex_oauth",
      device_code: "device-1",
      user_code: "ABCD-EFGH",
      verification_uri: "https://example.com/device",
      expires_in: 600,
      interval: 5,
    });
    apiMocks.authPollForAccount.mockImplementation(() => new Promise(() => {}));
    const { result } = renderHook(() => useManagedAuth("codex_oauth"), {
      wrapper: createWrapper(),
    });
    act(() => result.current.reauthAccount("acct-1"));
    await waitFor(() => expect(result.current.deviceCode).not.toBeNull());

    act(() => result.current.cancelAuth());

    await waitFor(() =>
      expect(apiMocks.authCancelLogin).toHaveBeenCalledWith(
        "codex_oauth",
        "device-1",
      ),
    );
    expect(result.current.pollingState).toBe("idle");
  });

  it("refreshes status when login committed before cancellation", async () => {
    let resolvePoll!: (account: object) => void;
    let resolveCancel!: (cancelled: boolean) => void;
    apiMocks.authStartLogin.mockResolvedValue({
      provider: "codex_oauth",
      device_code: "device-1",
      user_code: "ABCD-EFGH",
      verification_uri: "https://example.com/device",
      expires_in: 600,
      interval: 5,
    });
    apiMocks.authPollForAccount.mockImplementation(
      () => new Promise((resolve) => (resolvePoll = resolve)),
    );
    apiMocks.authCancelLogin.mockImplementation(
      () => new Promise((resolve) => (resolveCancel = resolve)),
    );
    const { result } = renderHook(() => useManagedAuth("codex_oauth"), {
      wrapper: createWrapper(),
    });
    await waitFor(() => expect(result.current.isStatusSuccess).toBe(true));

    act(() => result.current.reauthAccount("acct-1"));
    await waitFor(() => expect(apiMocks.authPollForAccount).toHaveBeenCalled());
    apiMocks.authGetStatus.mockResolvedValue({
      provider: "codex_oauth",
      authenticated: true,
      default_account_id: "acct-1",
      accounts: [
        ...result.current.accounts,
        {
          id: "acct-2",
          provider: "codex_oauth",
          login: "other@example.com",
          avatar_url: null,
          authenticated_at: 2,
          is_default: false,
          github_domain: "",
          reauth_required: false,
          requires_reauth: false,
        },
      ],
    });

    act(() => result.current.cancelAuth());
    await waitFor(() => expect(apiMocks.authCancelLogin).toHaveBeenCalled());
    act(() => {
      resolvePoll({ id: "acct-2" });
      resolveCancel(false);
    });

    await waitFor(() => expect(result.current.accounts).toHaveLength(2));
  });

  it("waits for active-flow cancellation before starting another login", async () => {
    let resolveCancel!: (cancelled: boolean) => void;
    apiMocks.authStartLogin.mockResolvedValue({
      provider: "codex_oauth",
      device_code: "device-1",
      user_code: "ABCD-EFGH",
      verification_uri: "https://example.com/device",
      expires_in: 600,
      interval: 5,
    });
    apiMocks.authPollForAccount.mockImplementation(() => new Promise(() => {}));
    apiMocks.authCancelLogin.mockImplementation(
      () => new Promise((resolve) => (resolveCancel = resolve)),
    );
    const { result } = renderHook(() => useManagedAuth("codex_oauth"), {
      wrapper: createWrapper(),
    });

    act(() => result.current.reauthAccount("acct-1"));
    await waitFor(() => expect(result.current.deviceCode).not.toBeNull());
    act(() => result.current.reauthAccount("acct-1"));
    await waitFor(() => expect(apiMocks.authCancelLogin).toHaveBeenCalled());
    expect(apiMocks.authStartLogin).toHaveBeenCalledTimes(1);

    act(() => resolveCancel(true));

    await waitFor(() =>
      expect(apiMocks.authStartLogin).toHaveBeenCalledTimes(2),
    );
  });

  it.each([
    ["remove", false],
    ["remove", true],
    ["logout", false],
    ["logout", true],
  ] as const)(
    "refreshes providers and accounts after %s (partial failure=%s)",
    async (operation, fails) => {
      const queryClient = new QueryClient({
        defaultOptions: {
          queries: { retry: false },
          mutations: { retry: false },
        },
      });
      queryClient.setQueryData(["providers", "codex"], {
        current: "old-binding",
      });
      queryClient.setQueryData(["providers", "claude"], {
        current: "unrelated",
      });
      const { result } = renderHook(() => useManagedAuth("codex_oauth"), {
        wrapper: createWrapper(queryClient),
      });
      await waitFor(() => expect(result.current.accounts).toHaveLength(1));
      apiMocks.authGetStatus.mockResolvedValue({
        provider: "codex_oauth",
        authenticated: false,
        accounts: [],
      });
      if (fails) {
        const mutation =
          operation === "remove"
            ? apiMocks.authRemoveAccount
            : apiMocks.authLogout;
        mutation.mockRejectedValue(
          new Error("账号已删除，但 Codex 供应商解绑失败"),
        );
      }
      act(() =>
        operation === "remove"
          ? result.current.removeAccount("acct-1")
          : result.current.logout(),
      );
      await waitFor(() =>
        expect(
          queryClient.getQueryState(["providers", "codex"])?.isInvalidated,
        ).toBe(true),
      );
      await waitFor(() => expect(result.current.accounts).toHaveLength(0));
      expect(
        queryClient.getQueryState(["providers", "claude"])?.isInvalidated,
      ).toBe(false);
      if (fails) expect(result.current.error).toContain("解绑失败");
    },
  );

  it("shows a success toast after removing an account", async () => {
    const { result } = renderHook(() => useManagedAuth("codex_oauth"), {
      wrapper: createWrapper(),
    });
    await waitFor(() => expect(result.current.isStatusSuccess).toBe(true));

    act(() => result.current.removeAccount("acct-1"));

    await waitFor(() =>
      expect(apiMocks.authRemoveAccount).toHaveBeenCalledWith(
        "codex_oauth",
        "acct-1",
      ),
    );
    await waitFor(() =>
      expect(toastMocks.success).toHaveBeenCalledWith("账号已移除"),
    );
  });
});
