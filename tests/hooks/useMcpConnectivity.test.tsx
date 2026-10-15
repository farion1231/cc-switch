import { StrictMode, type PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useMcpConnectivity } from "@/hooks/useMcpConnectivity";
import { mcpApi, type McpConnectivityResult } from "@/lib/api/mcp";
import type { McpServersMap } from "@/types";

vi.mock("@/lib/api/mcp", () => ({
  mcpApi: {
    startConnectivityChecks: vi.fn(),
    cancelConnectivityChecks: vi.fn(),
  },
}));

const servers = (enabled = true, command = "example"): McpServersMap => ({
  example: {
    id: "example",
    name: "Example",
    server: { command },
    apps: {
      claude: enabled,
      codex: false,
      gemini: false,
      opencode: false,
      openclaw: false,
      hermes: false,
    },
  },
});

describe("page-local MCP connectivity", () => {
  beforeEach(() => {
    vi.mocked(mcpApi.startConnectivityChecks)
      .mockReset()
      .mockResolvedValue("batch");
    vi.mocked(mcpApi.cancelConnectivityChecks).mockReset().mockResolvedValue();
  });

  it("waits for configured, enabled servers and never starts for an empty page", () => {
    const { rerender } = renderHook(({ data }) => useMcpConnectivity(data), {
      initialProps: { data: undefined as McpServersMap | undefined },
    });
    rerender({ data: {} });
    rerender({ data: servers(false) });
    expect(mcpApi.startConnectivityChecks).not.toHaveBeenCalled();
    rerender({ data: servers() });
    expect(mcpApi.startConnectivityChecks).toHaveBeenCalledWith(
      ["example"],
      expect.any(Function),
    );
    expect(mcpApi.startConnectivityChecks).toHaveBeenCalledTimes(1);
  });

  it("keeps results through metadata changes and focus/online events without reconnecting", () => {
    const { result, rerender } = renderHook(
      ({ data }) => useMcpConnectivity(data),
      { initialProps: { data: servers() } },
    );
    const deliver = vi.mocked(mcpApi.startConnectivityChecks).mock.calls[0][1];
    act(() => deliver({ serverId: "example", connected: true }));
    expect(result.current.example).toBe("connected");
    const data = servers();
    data.example.description = "Updated description";
    data.example.apps.codex = true;
    rerender({ data });
    act(() => {
      window.dispatchEvent(new Event("focus"));
      window.dispatchEvent(new Event("online"));
    });
    expect(mcpApi.startConnectivityChecks).toHaveBeenCalledTimes(1);
    expect(result.current.example).toBe("connected");
  });

  it("cancels stale configs and ignores their late results", async () => {
    const { result, rerender, unmount } = renderHook(
      ({ data }) => useMcpConnectivity(data),
      { initialProps: { data: servers() } },
    );
    const oldResult = vi.mocked(mcpApi.startConnectivityChecks).mock
      .calls[0][1];
    act(() => oldResult({ serverId: "example", connected: true }));
    rerender({ data: servers(true, "changed") });
    expect(result.current.example).toBeUndefined();
    act(() => oldResult({ serverId: "example", connected: true }));
    expect(result.current.example).toBeUndefined();
    const newResult = vi.mocked(mcpApi.startConnectivityChecks).mock
      .calls[1][1];
    act(() => newResult({ serverId: "example", connected: false }));
    expect(result.current.example).toBe("disconnected");
    await waitFor(() =>
      expect(mcpApi.cancelConnectivityChecks).toHaveBeenCalledTimes(1),
    );
    unmount();
    await waitFor(() =>
      expect(mcpApi.cancelConnectivityChecks).toHaveBeenCalledTimes(2),
    );
  });

  it("cancels even when leaving before the backend registers the batch", async () => {
    let register!: (id: string) => void;
    vi.mocked(mcpApi.startConnectivityChecks).mockReturnValueOnce(
      new Promise((resolve) => {
        register = resolve;
      }),
    );
    const { unmount } = renderHook(() => useMcpConnectivity(servers()));
    unmount();
    await act(async () => register("late-batch"));
    expect(mcpApi.cancelConnectivityChecks).toHaveBeenCalledWith("late-batch");
  });

  it("shows failures without retrying and starts a fresh batch on re-entry", async () => {
    vi.mocked(mcpApi.startConnectivityChecks).mockRejectedValueOnce(
      new Error("IPC failed"),
    );
    const first = renderHook(() => useMcpConnectivity(servers()));
    await waitFor(() =>
      expect(first.result.current.example).toBe("disconnected"),
    );
    expect(mcpApi.startConnectivityChecks).toHaveBeenCalledTimes(1);
    first.unmount();
    const second = renderHook(() => useMcpConnectivity(servers()));
    expect(second.result.current.example).toBeUndefined();
    expect(mcpApi.startConnectivityChecks).toHaveBeenCalledTimes(2);
  });

  it("cleans up StrictMode's discarded batch", async () => {
    let batch = 0;
    const callbacks: ((result: McpConnectivityResult) => void)[] = [];
    vi.mocked(mcpApi.startConnectivityChecks).mockImplementation(
      async (_ids, onResult) => {
        callbacks.push(onResult);
        return `batch-${++batch}`;
      },
    );
    const { result, unmount } = renderHook(
      () => useMcpConnectivity(servers()),
      {
        wrapper: ({ children }: PropsWithChildren) => (
          <StrictMode>{children}</StrictMode>
        ),
      },
    );
    await waitFor(() =>
      expect(mcpApi.cancelConnectivityChecks).toHaveBeenCalledWith("batch-1"),
    );
    act(() => callbacks[0]({ serverId: "example", connected: true }));
    expect(result.current.example).toBeUndefined();
    unmount();
    await waitFor(() =>
      expect(mcpApi.cancelConnectivityChecks).toHaveBeenCalledWith("batch-2"),
    );
  });
});
