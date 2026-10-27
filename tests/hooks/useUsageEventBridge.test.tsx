import type { PropsWithChildren } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
import { usageKeys } from "@/lib/query/usage";
import { emitTauriEvent } from "../msw/tauriMocks";

describe("useUsageEventBridge", () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("coalesces event bursts into one invalidation per window, trailing edge keeps the last one", async () => {
    const client = new QueryClient();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { unmount } = renderHook(() => useUsageEventBridge(), { wrapper });
    await act(async () => {});

    // 冷却期内的连续事件只触发一次立即失效
    act(() => emitTauriEvent("usage-log-recorded", null));
    act(() => emitTauriEvent("usage-log-recorded", null));
    act(() => emitTauriEvent("usage-log-recorded", null));
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: usageKeys.all });

    // 冷却期结束前的后续事件合并成一次尾沿刷新，最后一次不丢
    act(() => {
      vi.advanceTimersByTime(4_000);
      emitTauriEvent("usage-log-recorded", null);
    });
    expect(invalidate).toHaveBeenCalledTimes(1);
    act(() => {
      vi.advanceTimersByTime(1_000);
    });
    expect(invalidate).toHaveBeenCalledTimes(2);
    expect(invalidate).toHaveBeenLastCalledWith({ queryKey: usageKeys.all });

    unmount();
    // 卸载后不再响应事件
    act(() => emitTauriEvent("usage-log-recorded", null));
    expect(invalidate).toHaveBeenCalledTimes(2);
    client.clear();
  });
});
