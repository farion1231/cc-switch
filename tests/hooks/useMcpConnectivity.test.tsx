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

const delivery = (call = 0) =>
  vi.mocked(mcpApi.startConnectivityChecks).mock.calls[call][1];

describe("page-local MCP connectivity", () => {
  beforeEach(() => {
    vi.mocked(mcpApi.startConnectivityChecks)
      .mockReset()
      .mockResolvedValue("batch");
    vi.mocked(mcpApi.cancelConnectivityChecks).mockReset().mockResolvedValue();
  });

  it("never checks on its own and shows enabled servers as unchecked", () => {
    const { result, rerender } = renderHook(
      ({ data }) => useMcpConnectivity(data),
      { initialProps: { data: undefined as McpServersMap | undefined } },
    );
    expect(result.current.canCheck).toBe(false);
    rerender({ data: servers(false) });
    expect(result.current.states).toEqual({});
    expect(result.current.canCheck).toBe(false);
    rerender({ data: servers() });
    act(() => {
      window.dispatchEvent(new Event("focus"));
      window.dispatchEvent(new Event("online"));
    });
    expect(mcpApi.startConnectivityChecks).not.toHaveBeenCalled();
    expect(result.current.states.example).toBe("unchecked");
    expect(result.current.checking).toBe(false);
    expect(result.current.canCheck).toBe(true);
  });

  it("checks every enabled server on request and reports progress", () => {
    const disabled = { ...servers(false).example, id: "disabled" };
    const { result } = renderHook(() =>
      useMcpConnectivity({ ...servers(), disabled }),
    );
    act(() => result.current.checkAll());
    expect(mcpApi.startConnectivityChecks).toHaveBeenCalledWith(
      ["example"],
      expect.any(Function),
    );
    expect(result.current.states.example).toBe("checking");
    expect(result.current.checking).toBe(true);
    act(() => delivery()({ serverId: "example", connected: true }));
    expect(result.current.states.example).toBe("connected");
    expect(result.current.checking).toBe(false);
  });

  it("checks a single server and ignores disabled ones", () => {
    const { result, rerender } = renderHook(
      ({ data }) => useMcpConnectivity(data),
      { initialProps: { data: servers(false) } },
    );
    act(() => result.current.checkServer("example"));
    expect(mcpApi.startConnectivityChecks).not.toHaveBeenCalled();
    rerender({ data: servers() });
    act(() => result.current.checkServer("example"));
    expect(mcpApi.startConnectivityChecks).toHaveBeenCalledWith(
      ["example"],
      expect.any(Function),
    );
    act(() => delivery()({ serverId: "example", connected: false }));
    expect(result.current.states.example).toBe("disconnected");
  });

  it("keeps results through metadata changes and resets them when the config changes", () => {
    const { result, rerender } = renderHook(
      ({ data }) => useMcpConnectivity(data),
      { initialProps: { data: servers() } },
    );
    act(() => result.current.checkAll());
    act(() => delivery()({ serverId: "example", connected: true }));
    const data = servers();
    data.example.name = "Renamed";
    data.example.apps.codex = true;
    rerender({ data });
    expect(result.current.states.example).toBe("connected");
    rerender({ data: servers(true, "changed") });
    expect(result.current.states.example).toBe("unchecked");
    expect(mcpApi.startConnectivityChecks).toHaveBeenCalledTimes(1);
  });

  it("replaces running checks and ignores their late results", async () => {
    let batch = 0;
    vi.mocked(mcpApi.startConnectivityChecks).mockImplementation(
      async () => `batch-${++batch}`,
    );
    const { result } = renderHook(() => useMcpConnectivity(servers()));
    act(() => result.current.checkAll());
    act(() => result.current.checkAll());
    await waitFor(() =>
      expect(mcpApi.cancelConnectivityChecks).toHaveBeenCalledWith("batch-1"),
    );
    act(() => delivery(0)({ serverId: "example", connected: true }));
    expect(result.current.states.example).toBe("checking");
    act(() => delivery(1)({ serverId: "example", connected: false }));
    expect(result.current.states.example).toBe("disconnected");
  });

  it("lets a newer single-server check win over an older batch", () => {
    const { result } = renderHook(() => useMcpConnectivity(servers()));
    act(() => result.current.checkAll());
    act(() => result.current.checkServer("example"));
    act(() => delivery(1)({ serverId: "example", connected: true }));
    act(() => delivery(0)({ serverId: "example", connected: false }));
    expect(result.current.states.example).toBe("connected");
  });

  it("cancels running checks when leaving, even before the backend registers them", async () => {
    let register!: (id: string) => void;
    vi.mocked(mcpApi.startConnectivityChecks).mockReturnValueOnce(
      new Promise((resolve) => {
        register = resolve;
      }),
    );
    const { result, unmount } = renderHook(() => useMcpConnectivity(servers()));
    act(() => result.current.checkAll());
    unmount();
    await act(async () => register("late-batch"));
    expect(mcpApi.cancelConnectivityChecks).toHaveBeenCalledWith("late-batch");
  });

  it("does not cancel batches that already finished", async () => {
    const { result, unmount } = renderHook(() => useMcpConnectivity(servers()));
    act(() => result.current.checkAll());
    act(() => delivery()({ serverId: "example", connected: true }));
    unmount();
    await Promise.resolve();
    expect(mcpApi.cancelConnectivityChecks).not.toHaveBeenCalled();
  });

  it("shows a failed start as disconnected without retrying", async () => {
    vi.mocked(mcpApi.startConnectivityChecks).mockRejectedValueOnce(
      new Error("IPC failed"),
    );
    const { result } = renderHook(() => useMcpConnectivity(servers()));
    act(() => result.current.checkAll());
    await waitFor(() =>
      expect(result.current.states.example).toBe("disconnected"),
    );
    expect(mcpApi.startConnectivityChecks).toHaveBeenCalledTimes(1);
  });

  it("starts nothing under StrictMode until asked", () => {
    const callbacks: ((result: McpConnectivityResult) => void)[] = [];
    vi.mocked(mcpApi.startConnectivityChecks).mockImplementation(
      async (_ids, onResult) => {
        callbacks.push(onResult);
        return "batch";
      },
    );
    const { result } = renderHook(() => useMcpConnectivity(servers()), {
      wrapper: ({ children }: PropsWithChildren) => (
        <StrictMode>{children}</StrictMode>
      ),
    });
    expect(callbacks).toHaveLength(0);
    act(() => result.current.checkAll());
    expect(callbacks).toHaveLength(1);
    act(() => callbacks[0]({ serverId: "example", connected: true }));
    expect(result.current.states.example).toBe("connected");
  });
});
