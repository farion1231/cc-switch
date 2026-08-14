/** @fileoverview Polling and refresh-policy contracts for shared managed OAuth. */

import { createElement, type ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { managedAuthStatusRefetchInterval } from "@/components/providers/forms/hooks/useManagedAuth";
import { useManagedAuth } from "@/components/providers/forms/hooks/useManagedAuth";

const apiMocks = vi.hoisted(() => ({
  authGetStatus: vi.fn(),
  authPollForAccount: vi.fn(),
  authStartLogin: vi.fn(),
  authCancelLogin: vi.fn(),
  authRemoveAccount: vi.fn(),
  openExternal: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  authApi: {
    authGetStatus: (...args: unknown[]) => apiMocks.authGetStatus(...args),
    authPollForAccount: (...args: unknown[]) =>
      apiMocks.authPollForAccount(...args),
    authStartLogin: (...args: unknown[]) => apiMocks.authStartLogin(...args),
    authCancelLogin: (...args: unknown[]) => apiMocks.authCancelLogin(...args),
    authRemoveAccount: (...args: unknown[]) =>
      apiMocks.authRemoveAccount(...args),
  },
  settingsApi: {
    openExternal: (...args: unknown[]) => apiMocks.openExternal(...args),
  },
}));

vi.mock("@/lib/clipboard", () => ({
  copyText: vi.fn().mockResolvedValue(undefined),
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
  return { wrapper };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

beforeEach(() => {
  apiMocks.authGetStatus.mockReset().mockResolvedValue({
    provider: "kimi_oauth",
    authenticated: false,
    default_account_id: null,
    accounts: [],
  });
  apiMocks.authStartLogin.mockReset().mockResolvedValue({
    provider: "kimi_oauth",
    device_code: "device-code",
    user_code: "user-code",
    verification_uri: "https://example.com/device",
    expires_in: 300,
    interval: 5,
  });
  apiMocks.authPollForAccount.mockReset();
  apiMocks.authCancelLogin.mockReset().mockResolvedValue(undefined);
  apiMocks.authRemoveAccount.mockReset().mockResolvedValue(undefined);
  apiMocks.openExternal.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("managedAuthStatusRefetchInterval", () => {
  it("refreshes providers whose proxy hot path can persist reauthentication state", () => {
    expect(managedAuthStatusRefetchInterval("xai_oauth")).toBe(15_000);
    expect(managedAuthStatusRefetchInterval("kimi_oauth")).toBe(15_000);
  });

  it("leaves providers without hot-path status transitions event-driven", () => {
    expect(managedAuthStatusRefetchInterval("github_copilot")).toBe(false);
    expect(managedAuthStatusRefetchInterval("codex_oauth")).toBe(false);
  });
});

describe("useManagedAuth device polling", () => {
  it("does not start another poll while the previous request is pending", async () => {
    vi.useFakeTimers();
    const pendingPoll = deferred<null>();
    apiMocks.authPollForAccount.mockReturnValue(pendingPoll.promise);
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useManagedAuth("kimi_oauth"), {
      wrapper,
    });

    await act(async () => {
      result.current.startAuth();
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(24_000);
    });

    expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(1);
    pendingPoll.resolve(null);
    apiMocks.authPollForAccount.mockResolvedValue(null);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(8_000);
    });
    expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(2);
  });

  it("polls a replacement login while the canceled request is still pending", async () => {
    vi.useFakeTimers();
    const stalePoll = deferred<null>();
    const firstResponse = {
      provider: "kimi_oauth",
      device_code: "first-device-code",
      user_code: "first-user-code",
      verification_uri: "https://example.com/device",
      expires_in: 300,
      interval: 5,
    };
    apiMocks.authStartLogin
      .mockResolvedValueOnce(firstResponse)
      .mockResolvedValueOnce({
        ...firstResponse,
        device_code: "replacement-device-code",
        user_code: "replacement-user-code",
      });
    apiMocks.authPollForAccount
      .mockReturnValueOnce(stalePoll.promise)
      .mockResolvedValue(null);
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useManagedAuth("kimi_oauth"), {
      wrapper,
    });

    await act(async () => {
      result.current.startAuth();
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(apiMocks.authPollForAccount).toHaveBeenNthCalledWith(
      1,
      "kimi_oauth",
      "first-device-code",
      undefined,
    );

    act(() => {
      result.current.cancelAuth();
      result.current.startAuth();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    expect(apiMocks.authPollForAccount).toHaveBeenNthCalledWith(
      2,
      "kimi_oauth",
      "replacement-device-code",
      undefined,
    );
    stalePoll.resolve(null);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
  });

  it("ignores a pending poll failure after authentication is canceled", async () => {
    vi.useFakeTimers();
    const stalePoll = deferred<null>();
    apiMocks.authPollForAccount.mockReturnValue(stalePoll.promise);
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useManagedAuth("kimi_oauth"), {
      wrapper,
    });

    await act(async () => {
      result.current.startAuth();
      await vi.advanceTimersByTimeAsync(0);
    });
    act(() => {
      result.current.cancelAuth();
    });

    stalePoll.reject(new Error("Device Code does not exist"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    expect(result.current.pollingState).toBe("idle");
    expect(result.current.error).toBeNull();
  });

  it("does not install polling after browser setup is canceled", async () => {
    vi.useFakeTimers();
    const pendingBrowser = deferred<void>();
    apiMocks.openExternal.mockReturnValue(pendingBrowser.promise);
    apiMocks.authPollForAccount.mockResolvedValue(null);
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useManagedAuth("kimi_oauth"), {
      wrapper,
    });

    await act(async () => {
      result.current.startAuth();
      await vi.advanceTimersByTimeAsync(0);
    });
    act(() => {
      result.current.cancelAuth();
    });

    pendingBrowser.resolve();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    expect(apiMocks.authPollForAccount).not.toHaveBeenCalled();
    expect(result.current.pollingState).toBe("idle");
    expect(result.current.error).toBeNull();
  });

  it("tells the backend to drop the pending device code on cancel", async () => {
    vi.useFakeTimers();
    apiMocks.authPollForAccount.mockResolvedValue(null);
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useManagedAuth("kimi_oauth"), {
      wrapper,
    });

    await act(async () => {
      result.current.startAuth();
      await vi.advanceTimersByTimeAsync(0);
    });
    act(() => {
      result.current.cancelAuth();
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(apiMocks.authCancelLogin).toHaveBeenCalledWith(
      "kimi_oauth",
      "device-code",
    );
  });

  it("cancels the previous backend login when starting a replacement", async () => {
    vi.useFakeTimers();
    const firstResponse = {
      provider: "kimi_oauth",
      device_code: "first-device-code",
      user_code: "first-user-code",
      verification_uri: "https://example.com/device",
      expires_in: 300,
      interval: 5,
    };
    apiMocks.authStartLogin
      .mockResolvedValueOnce(firstResponse)
      .mockResolvedValueOnce({
        ...firstResponse,
        device_code: "replacement-device-code",
        user_code: "replacement-user-code",
      });
    apiMocks.authPollForAccount.mockResolvedValue(null);
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useManagedAuth("kimi_oauth"), {
      wrapper,
    });

    await act(async () => {
      result.current.startAuth();
      await vi.advanceTimersByTimeAsync(0);
    });
    act(() => {
      result.current.startAuth();
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(apiMocks.authCancelLogin).toHaveBeenCalledWith(
      "kimi_oauth",
      "first-device-code",
    );
  });

  it("cancels a pending backend login on unmount", async () => {
    vi.useFakeTimers();
    apiMocks.authPollForAccount.mockResolvedValue(null);
    const { wrapper } = createWrapper();
    const { result, unmount } = renderHook(() => useManagedAuth("kimi_oauth"), {
      wrapper,
    });

    await act(async () => {
      result.current.startAuth();
      await vi.advanceTimersByTimeAsync(0);
    });
    unmount();
    await act(async () => {
      await Promise.resolve();
    });

    expect(apiMocks.authCancelLogin).toHaveBeenCalledWith(
      "kimi_oauth",
      "device-code",
    );
  });

  it("cancels the backend login when the device code expires", async () => {
    vi.useFakeTimers();
    apiMocks.authPollForAccount.mockResolvedValue(null);
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useManagedAuth("kimi_oauth"), {
      wrapper,
    });

    await act(async () => {
      result.current.startAuth();
      await vi.advanceTimersByTimeAsync(0);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300_000);
    });

    expect(apiMocks.authCancelLogin).toHaveBeenCalledWith(
      "kimi_oauth",
      "device-code",
    );
    expect(result.current.pollingState).toBe("error");
  });

  it("cancels a pending backend login when removing an account", async () => {
    vi.useFakeTimers();
    apiMocks.authPollForAccount.mockResolvedValue(null);
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useManagedAuth("kimi_oauth"), {
      wrapper,
    });

    await act(async () => {
      result.current.startAuth();
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.removeAccount("account-one");
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(apiMocks.authCancelLogin).toHaveBeenCalledWith(
      "kimi_oauth",
      "device-code",
    );
    expect(apiMocks.authRemoveAccount).toHaveBeenCalledWith(
      "kimi_oauth",
      "account-one",
    );
    expect(result.current.pollingState).toBe("idle");
    expect(result.current.deviceCode).toBeNull();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(24_000);
    });
    expect(apiMocks.authPollForAccount).toHaveBeenCalledTimes(1);
  });
});
