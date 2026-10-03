import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
import { usageKeys } from "@/lib/query/usage";

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(),
  unlisten: vi.fn(),
  handler: undefined as ((event?: unknown) => void) | undefined,
}));

const queryClientMocks = vi.hoisted(() => ({
  invalidateQueries: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => eventMocks.listen(...args),
}));

vi.mock("@tanstack/react-query", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tanstack/react-query")>();
  return {
    ...actual,
    useQueryClient: () => ({
      invalidateQueries: queryClientMocks.invalidateQueries,
    }),
  };
});

describe("useUsageEventBridge", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-20T00:00:00Z"));
    eventMocks.listen.mockReset();
    eventMocks.unlisten.mockReset();
    eventMocks.handler = undefined;
    queryClientMocks.invalidateQueries.mockReset();
    queryClientMocks.invalidateQueries.mockResolvedValue(undefined);
    eventMocks.listen.mockImplementation(
      async (_event: string, handler: (event?: unknown) => void) => {
        eventMocks.handler = handler;
        return eventMocks.unlisten;
      },
    );
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("coalesces bursts into at most one active-query refresh every five seconds", async () => {
    const { unmount } = renderHook(() => useUsageEventBridge());

    await act(async () => {
      await Promise.resolve();
    });

    act(() => {
      eventMocks.handler?.();
      eventMocks.handler?.();
      eventMocks.handler?.();
    });

    expect(queryClientMocks.invalidateQueries).toHaveBeenCalledTimes(1);
    expect(queryClientMocks.invalidateQueries).toHaveBeenCalledWith({
      queryKey: usageKeys.all,
      refetchType: "active",
    });

    act(() => {
      vi.advanceTimersByTime(4999);
    });
    expect(queryClientMocks.invalidateQueries).toHaveBeenCalledTimes(1);

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(queryClientMocks.invalidateQueries).toHaveBeenCalledTimes(2);

    unmount();
    expect(eventMocks.unlisten).toHaveBeenCalledOnce();
  });

  it("does not subscribe when automatic refresh is disabled", async () => {
    renderHook(() => useUsageEventBridge(false));

    await act(async () => {
      await Promise.resolve();
    });

    expect(eventMocks.listen).not.toHaveBeenCalled();
  });

  it("cancels a pending trailing refresh when unmounted", async () => {
    const { unmount } = renderHook(() => useUsageEventBridge());

    await act(async () => {
      await Promise.resolve();
    });

    act(() => {
      eventMocks.handler?.();
      eventMocks.handler?.();
    });
    unmount();

    act(() => {
      vi.advanceTimersByTime(5000);
    });

    expect(queryClientMocks.invalidateQueries).toHaveBeenCalledOnce();
  });
});
