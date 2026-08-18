import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { describe, expect, it, vi } from "vitest";

import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
import { usageKeys } from "@/lib/query/usage";
import { emitTauriEvent } from "../msw/tauriMocks";

describe("useUsageEventBridge", () => {
  it("invalidates usage queries after a usage event", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const invalidateQueries = vi.spyOn(client, "invalidateQueries");
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );

    renderHook(() => useUsageEventBridge(), { wrapper });

    await act(async () => {
      await Promise.resolve();
    });
    act(() => emitTauriEvent("usage-log-recorded", undefined));

    await waitFor(() =>
      expect(invalidateQueries).toHaveBeenCalledWith({
        queryKey: usageKeys.all,
      }),
    );
  });
});
