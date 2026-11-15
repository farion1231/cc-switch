import type { PropsWithChildren } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import {
  focusManager,
  QueryClient,
  QueryClientProvider,
  useQuery,
} from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { waitFor } from "@testing-library/react";
import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
import { usageKeys } from "@/lib/query/usage";
import { emitTauriEvent } from "../msw/tauriMocks";

describe("useUsageEventBridge", () => {
  beforeEach(() => focusManager.setFocused(true));
  afterEach(() => {
    cleanup();
    focusManager.setFocused(undefined);
  });

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
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: usageKeys.all,
      refetchType: "active",
    });
    unmount();
    act(() => emitTauriEvent("usage-log-recorded", null));
    expect(invalidate).toHaveBeenCalledTimes(2);
    client.clear();
  });

  it("keeps background data stale and fetches once on return", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const query = vi.fn().mockResolvedValue(1);
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result, unmount } = renderHook(
      () => {
        useUsageEventBridge();
        return useQuery({
          queryKey: [...usageKeys.all, "summary"],
          queryFn: query,
          staleTime: Infinity,
        });
      },
      { wrapper },
    );
    await waitFor(() => expect(result.current.data).toBe(1));
    act(() => focusManager.setFocused(false));
    query.mockResolvedValue(2);
    await act(async () => {
      for (let i = 0; i < 10; i++) emitTauriEvent("usage-log-recorded", null);
    });
    expect(query).toHaveBeenCalledTimes(1);
    expect(
      client.getQueryState([...usageKeys.all, "summary"])?.isInvalidated,
    ).toBe(true);
    act(() => focusManager.setFocused(true));
    await waitFor(() => expect(result.current.data).toBe(2));
    expect(query).toHaveBeenCalledTimes(2);
    unmount();
    client.clear();
  });
});
