import type { PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  useDeleteVpsServer,
  useSaveVpsServer,
  useVpsServers,
  vpsKeys,
} from "@/hooks/useVps";
import type { VpsServer } from "@/lib/api/vps";

const mocks = vi.hoisted(() => ({
  getServers: vi.fn(),
  saveServer: vi.fn(),
  deleteServer: vi.fn(),
}));
vi.mock("@/lib/api/vps", () => ({ vpsApi: mocks }));

const host = { id: "host-id", name: "Saved host" } as VpsServer;
function wrapper(client: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
  };
}

describe("VPS mutation hooks", () => {
  beforeEach(() => vi.resetAllMocks());

  it("updates the global list only after backend success and refreshes Skill discovery", async () => {
    const client = new QueryClient();
    client.setQueryData(vpsKeys.servers, []);
    const invalidate = vi
      .spyOn(client, "invalidateQueries")
      .mockResolvedValue(undefined);
    mocks.saveServer.mockResolvedValue([host]);
    const { result } = renderHook(useSaveVpsServer, {
      wrapper: wrapper(client),
    });
    await act(async () => {
      await result.current.mutateAsync({ server: host });
    });
    expect(client.getQueryData(vpsKeys.servers)).toEqual([host]);
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["skills", "installed"],
    });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["skills", "unmanaged"],
    });
  });

  it("does not turn a failed deployment into a successful binding in the cache", async () => {
    const client = new QueryClient();
    const original = { ...host, name: "Before" };
    client.setQueryData(vpsKeys.servers, [original]);
    mocks.saveServer.mockRejectedValue(new Error("deployment failed"));
    const { result } = renderHook(useSaveVpsServer, {
      wrapper: wrapper(client),
    });
    await act(async () => {
      await expect(
        result.current.mutateAsync({ server: host }),
      ).rejects.toThrow("deployment failed");
    });
    expect(client.getQueryData(vpsKeys.servers)).toEqual([original]);
  });

  it.each(["save", "delete"] as const)(
    "keeps a failed %s pending until authoritative VPS reconciliation finishes",
    async (operation) => {
      const client = new QueryClient();
      const original = { ...host, name: "Before" };
      const committed = operation === "save" ? [host] : [];
      let reconcile!: (servers: VpsServer[]) => void;
      mocks.getServers.mockResolvedValueOnce([original]).mockReturnValueOnce(
        new Promise<VpsServer[]>((resolve) => {
          reconcile = resolve;
        }),
      );
      const failure = new Error("post-commit deployment failed");
      mocks.saveServer.mockRejectedValue(failure);
      mocks.deleteServer.mockRejectedValue(failure);
      const { result } = renderHook(
        () => ({
          query: useVpsServers(),
          save: useSaveVpsServer(),
          remove: useDeleteVpsServer(),
        }),
        { wrapper: wrapper(client) },
      );
      await waitFor(() =>
        expect(result.current.query.data).toEqual([original]),
      );
      let pending!: Promise<unknown>;
      act(() => {
        pending = (
          operation === "save"
            ? result.current.save.mutateAsync({ server: host })
            : result.current.remove.mutateAsync(host.id)
        ).catch((error) => error);
      });
      await waitFor(() => expect(mocks.getServers).toHaveBeenCalledTimes(2));
      expect(
        result.current[operation === "save" ? "save" : "remove"].isPending,
      ).toBe(true);
      expect(result.current.query.isFetching).toBe(true);
      expect(result.current.query.data).toEqual([original]);
      await act(async () => {
        reconcile(committed);
        expect(await pending).toBe(failure);
      });
      await waitFor(() => expect(result.current.query.data).toEqual(committed));
      expect(
        result.current[operation === "save" ? "save" : "remove"].isSuccess,
      ).toBe(false);
    },
  );

  it("keeps deletion pending until related cache invalidation finishes", async () => {
    const client = new QueryClient();
    client.setQueryData(vpsKeys.servers, [host]);
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    vi.spyOn(client, "invalidateQueries").mockReturnValue(pending);
    mocks.deleteServer.mockResolvedValue([]);
    const { result } = renderHook(useDeleteVpsServer, {
      wrapper: wrapper(client),
    });
    let operation!: Promise<unknown>;
    act(() => {
      operation = result.current.mutateAsync(host.id);
    });
    await waitFor(() => expect(result.current.isPending).toBe(true));
    release();
    await act(async () => {
      await operation;
    });
    await waitFor(() => expect(result.current.isPending).toBe(false));
    expect(client.getQueryData(vpsKeys.servers)).toEqual([]);
  });
});
