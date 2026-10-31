import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import { useCodexDesktopStatus } from "@/hooks/useCodexDesktopStatus";

const mocks = vi.hoisted(() => ({
  status: vi.fn(),
  windows: vi.fn(),
  tauri: vi.fn(),
  focus: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: mocks.tauri }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onFocusChanged: mocks.focus }),
}));
vi.mock("@/lib/api/codexDesktop", () => ({
  getCodexDesktopStatus: mocks.status,
}));
vi.mock("@/lib/platform", () => ({ isWindows: mocks.windows }));

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

describe("useCodexDesktopStatus", () => {
  afterEach(() => vi.useRealTimers());
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.windows.mockReturnValue(true);
    mocks.tauri.mockReturnValue(false);
    mocks.status.mockResolvedValue({
      supported: true,
      installed: true,
      running: false,
    });
  });
  it("reads actual desktop state on Windows", async () => {
    const { result } = renderHook(() => useCodexDesktopStatus(), { wrapper });
    await waitFor(() => expect(result.current.data?.installed).toBe(true));
    expect(result.current.data?.running).toBe(false);
    expect(mocks.status).toHaveBeenCalledTimes(1);
  });
  it("does not query Windows package discovery on macOS", () => {
    mocks.windows.mockReturnValue(false);
    renderHook(() => useCodexDesktopStatus(), { wrapper });
    expect(mocks.status).not.toHaveBeenCalled();
  });
  it("does not query when the Codex provider page is inactive", () => {
    renderHook(() => useCodexDesktopStatus(false), { wrapper });
    expect(mocks.status).not.toHaveBeenCalled();
  });
  it("does not poll and refreshes after focus returns", async () => {
    const { result, unmount } = renderHook(() => useCodexDesktopStatus(), {
      wrapper,
    });
    await waitFor(() => expect(result.current.data?.installed).toBe(true));
    vi.useFakeTimers();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(mocks.status).toHaveBeenCalledTimes(1);
    mocks.status.mockResolvedValue({
      supported: true,
      installed: true,
      running: true,
    });
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(mocks.status).toHaveBeenCalledTimes(2);
    expect(result.current.data?.running).toBe(true);
    unmount();
    window.dispatchEvent(new Event("focus"));
    expect(mocks.status).toHaveBeenCalledTimes(2);
  });
  it("uses native focus changes without keeping listeners after unmount", async () => {
    mocks.tauri.mockReturnValue(true);
    const off = vi.fn();
    let focus!: (event: { payload: boolean }) => void;
    mocks.focus.mockImplementation(async (handler) => {
      focus = handler;
      return off;
    });
    const { result, unmount } = renderHook(() => useCodexDesktopStatus(), {
      wrapper,
    });
    await waitFor(() => expect(result.current.data?.installed).toBe(true));
    act(() => focus({ payload: false }));
    expect(mocks.status).toHaveBeenCalledTimes(1);
    act(() => focus({ payload: true }));
    await waitFor(() => expect(mocks.status).toHaveBeenCalledTimes(2));
    unmount();
    expect(off).toHaveBeenCalledTimes(1);
    act(() => focus({ payload: true }));
    expect(mocks.status).toHaveBeenCalledTimes(2);
  });
  it("keeps browser focus refresh if native focus registration fails", async () => {
    mocks.tauri.mockReturnValue(true);
    mocks.focus.mockImplementation(() => {
      throw new Error("Native focus unavailable");
    });
    const { result } = renderHook(() => useCodexDesktopStatus(), { wrapper });
    await waitFor(() => expect(result.current.data?.installed).toBe(true));
    act(() => window.dispatchEvent(new Event("focus")));
    await waitFor(() => expect(mocks.status).toHaveBeenCalledTimes(2));
  });
});
