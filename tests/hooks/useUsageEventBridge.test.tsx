import type { PropsWithChildren } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
import { emitTauriEvent } from "../msw/tauriMocks";

describe("useUsageEventBridge", () => {
  afterEach(cleanup);

  it("stops event-driven refreshes while disabled and cleans up on unmount", async () => {
    const client = new QueryClient();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { rerender, unmount } = renderHook(
      ({ enabled }) => useUsageEventBridge(enabled),
      {
        wrapper,
        initialProps: { enabled: true },
      },
    );
    await act(async () => {});
    act(() => emitTauriEvent("usage-log-recorded", null));
    expect(invalidate).toHaveBeenCalledTimes(1);
    rerender({ enabled: false });
    act(() => emitTauriEvent("usage-log-recorded", null));
    expect(invalidate).toHaveBeenCalledTimes(1);
    rerender({ enabled: true });
    await act(async () => {});
    act(() => emitTauriEvent("usage-log-recorded", null));
    expect(invalidate).toHaveBeenCalledTimes(2);
    unmount();
    act(() => emitTauriEvent("usage-log-recorded", null));
    expect(invalidate).toHaveBeenCalledTimes(2);
    client.clear();
  });
});
