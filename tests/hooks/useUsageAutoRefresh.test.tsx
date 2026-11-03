import type { PropsWithChildren } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import {
  focusManager,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
  type MockInstance,
} from "vitest";
import { useUsageAutoRefresh } from "@/hooks/useUsageAutoRefresh";
import { usageApi } from "@/lib/api/usage";
import { usageKeys } from "@/lib/query/usage";
import type { SessionSyncResult } from "@/types/usage";

vi.mock("@/lib/api/usage", () => ({
  usageApi: { syncSessionUsage: vi.fn() },
}));

const syncResult: SessionSyncResult = {
  imported: 1,
  skipped: 0,
  filesScanned: 1,
  suspectedDuplicates: 0,
  deferredFiles: 0,
  errors: [],
};
const lastSyncKey = [...usageKeys.all, "session-last-sync"];
const syncMock = vi.mocked(usageApi.syncSessionUsage);
const advance = (ms: number) => act(() => vi.advanceTimersByTimeAsync(ms));
const flush = () => act(() => vi.advanceTimersByTimeAsync(0));

describe("useUsageAutoRefresh", () => {
  let client: QueryClient;
  let invalidate: MockInstance<QueryClient["invalidateQueries"]>;

  function wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
  }

  beforeEach(() => {
    vi.useFakeTimers();
    focusManager.setFocused(true);
    client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    invalidate = vi.spyOn(client, "invalidateQueries");
    syncMock.mockReset().mockResolvedValue(syncResult);
  });

  afterEach(() => {
    cleanup();
    client.clear();
    focusManager.setFocused(undefined);
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it.each([5000, 10000, 30000, 60000])(
    "scans on open, then every %i ms",
    async (refreshIntervalMs) => {
      renderHook(
        () =>
          useUsageAutoRefresh({
            refreshIntervalMs,
            sessionAutoSyncEnabled: true,
          }),
        { wrapper },
      );
      await flush();
      expect(syncMock).toHaveBeenCalledTimes(1);
      // 新用量由后端事件刷新，这里只刷新「上次同步」时间
      expect(invalidate).toHaveBeenCalledTimes(1);
      expect(invalidate).toHaveBeenCalledWith({ queryKey: lastSyncKey });
      await advance(refreshIntervalMs - 1);
      expect(syncMock).toHaveBeenCalledTimes(1);
      await advance(1);
      expect(syncMock).toHaveBeenCalledTimes(2);
      await advance(refreshIntervalMs);
      expect(syncMock).toHaveBeenCalledTimes(3);
    },
  );

  it("scans right away and reschedules when the interval changes", async () => {
    const { rerender } = renderHook(
      ({ interval }) =>
        useUsageAutoRefresh({
          refreshIntervalMs: interval,
          sessionAutoSyncEnabled: true,
        }),
      { wrapper, initialProps: { interval: 60000 } },
    );
    await flush();
    expect(syncMock).toHaveBeenCalledTimes(1);
    await advance(1000);
    rerender({ interval: 5000 });
    await flush();
    expect(syncMock).toHaveBeenCalledTimes(2);
    await advance(5000);
    expect(syncMock).toHaveBeenCalledTimes(3);
  });

  it("never scans while session auto-sync is off", async () => {
    const { rerender } = renderHook(
      ({ enabled }) =>
        useUsageAutoRefresh({
          refreshIntervalMs: 5000,
          sessionAutoSyncEnabled: enabled,
        }),
      { wrapper, initialProps: { enabled: false } },
    );
    await advance(20000);
    expect(syncMock).not.toHaveBeenCalled();
    expect(invalidate).not.toHaveBeenCalled();
    rerender({ enabled: true });
    await flush();
    expect(syncMock).toHaveBeenCalledTimes(1);
    rerender({ enabled: false });
    await advance(20000);
    expect(syncMock).toHaveBeenCalledTimes(1);
  });

  it("waits for a slow scan and shares it with manual sync without queueing more scans", async () => {
    let finish!: (result: SessionSyncResult) => void;
    syncMock.mockReturnValueOnce(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const { result } = renderHook(
      () =>
        useUsageAutoRefresh({
          refreshIntervalMs: 5000,
          sessionAutoSyncEnabled: true,
        }),
      { wrapper },
    );
    await flush();
    const manualSync = result.current.syncSessionUsage();
    await advance(20000);
    expect(syncMock).toHaveBeenCalledTimes(1);
    expect(invalidate).not.toHaveBeenCalled();
    await act(async () => {
      finish(syncResult);
      await manualSync;
    });
    expect(invalidate).toHaveBeenCalledTimes(1);
    await advance(5000);
    expect(syncMock).toHaveBeenCalledTimes(2);
  });

  it("skips hidden windows, rescans once visible, and pauses during a rebuild", async () => {
    focusManager.setFocused(false);
    const { rerender } = renderHook(
      ({ paused }) =>
        useUsageAutoRefresh({
          refreshIntervalMs: 5000,
          sessionAutoSyncEnabled: true,
          paused,
        }),
      { wrapper, initialProps: { paused: false } },
    );
    await advance(10000);
    expect(syncMock).not.toHaveBeenCalled();
    act(() => focusManager.setFocused(true));
    await flush();
    expect(syncMock).toHaveBeenCalledTimes(1);
    rerender({ paused: true });
    await advance(10000);
    expect(syncMock).toHaveBeenCalledTimes(1);
    rerender({ paused: false });
    await flush();
    expect(syncMock).toHaveBeenCalledTimes(2);
  });

  it.each(["disable", "unmount"] as const)(
    "ignores a pending scan after %s",
    async (action) => {
      let finish!: (result: SessionSyncResult) => void;
      syncMock.mockReturnValueOnce(
        new Promise((resolve) => {
          finish = resolve;
        }),
      );
      const { rerender, unmount } = renderHook(
        ({ enabled }) =>
          useUsageAutoRefresh({
            refreshIntervalMs: 5000,
            sessionAutoSyncEnabled: enabled,
          }),
        { wrapper, initialProps: { enabled: true } },
      );
      await flush();
      if (action === "disable") rerender({ enabled: false });
      else unmount();
      await act(async () => {
        finish(syncResult);
      });
      await advance(60000);
      expect(invalidate).not.toHaveBeenCalled();
      expect(syncMock).toHaveBeenCalledTimes(1);
    },
  );

  it("re-runs usage queries every minute for rolling ranges, even with scanning off", async () => {
    const { rerender } = renderHook(
      ({ rolling }) =>
        useUsageAutoRefresh({
          refreshIntervalMs: 5000,
          sessionAutoSyncEnabled: false,
          rollingRange: rolling,
        }),
      { wrapper, initialProps: { rolling: true } },
    );
    await advance(59_999);
    expect(invalidate).not.toHaveBeenCalled();
    await advance(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: usageKeys.all });
    focusManager.setFocused(false);
    await advance(60_000);
    expect(invalidate).toHaveBeenCalledTimes(1);
    focusManager.setFocused(true);
    rerender({ rolling: false });
    await advance(120_000);
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(syncMock).not.toHaveBeenCalled();
  });

  it("still updates the last sync time after a scan error and retries next time", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    syncMock.mockRejectedValueOnce(new Error("scan failed"));
    renderHook(
      () =>
        useUsageAutoRefresh({
          refreshIntervalMs: 5000,
          sessionAutoSyncEnabled: true,
        }),
      { wrapper },
    );
    await flush();
    expect(warn).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledTimes(1);
    await advance(5000);
    expect(syncMock).toHaveBeenCalledTimes(2);
    expect(invalidate).toHaveBeenCalledTimes(2);
  });
});
