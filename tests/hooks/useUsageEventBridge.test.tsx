import type { PropsWithChildren } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
import { usageKeys } from "@/lib/query/usage";
import { emitTauriEvent } from "../msw/tauriMocks";

describe("useUsageEventBridge", () => {
  afterEach(cleanup);

  it("refreshes usage queries on every backend event until unmount", async () => {
    const client = new QueryClient();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { unmount } = renderHook(() => useUsageEventBridge(), { wrapper });
    await act(async () => {});
    act(() => emitTauriEvent("usage-log-recorded", null));
    act(() => emitTauriEvent("usage-log-recorded", null));
    expect(invalidate).toHaveBeenCalledTimes(2);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: usageKeys.all });
    unmount();
    act(() => emitTauriEvent("usage-log-recorded", null));
    expect(invalidate).toHaveBeenCalledTimes(2);
    client.clear();
  });
});
