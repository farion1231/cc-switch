import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useBulkToggleMcpApp, useToggleMcpApp } from "@/hooks/useMcp";

const toggleAppMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api/mcp", () => ({
  mcpApi: {
    toggleApp: toggleAppMock,
  },
}));

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("MCP toggle hooks", () => {
  beforeEach(() => {
    toggleAppMock.mockReset();
  });

  it("runs bulk writes serially and invalidates the list once", async () => {
    let releaseFirst: (() => void) | undefined;
    let releaseInvalidation: (() => void) | undefined;
    const firstPending = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const invalidationPending = new Promise<void>((resolve) => {
      releaseInvalidation = resolve;
    });
    toggleAppMock.mockImplementation(async (serverId: string) => {
      if (serverId === "alpha") await firstPending;
    });
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockImplementation(() => invalidationPending);
    const { result } = renderHook(() => useBulkToggleMcpApp(), {
      wrapper: createWrapper(queryClient),
    });

    let mutation!: Promise<unknown>;
    act(() => {
      mutation = result.current.mutateAsync({
        serverIds: ["alpha", "beta"],
        app: "claude",
        enabled: true,
      });
    });

    await waitFor(() => expect(toggleAppMock).toHaveBeenCalledTimes(1));
    releaseFirst?.();
    await waitFor(() => expect(toggleAppMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(1));
    expect(result.current.isPending).toBe(true);
    releaseInvalidation?.();
    await act(async () => {
      await mutation;
    });

    expect(toggleAppMock.mock.calls).toEqual([
      ["alpha", "claude", true],
      ["beta", "claude", true],
    ]);
    expect(invalidateSpy).toHaveBeenCalledTimes(1);
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ["mcp", "all"] });
    await waitFor(() => expect(result.current.isPending).toBe(false));
  });

  it("refreshes the list when a single live-config write fails", async () => {
    toggleAppMock.mockRejectedValueOnce(new Error("write failed"));
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useToggleMcpApp(), {
      wrapper: createWrapper(queryClient),
    });

    await act(async () => {
      await expect(
        result.current.mutateAsync({
          serverId: "alpha",
          app: "claude",
          enabled: true,
        }),
      ).rejects.toThrow("write failed");
    });

    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ["mcp", "all"] });
  });
});
