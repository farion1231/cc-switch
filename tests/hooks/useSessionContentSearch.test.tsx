import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSessionContentSearch } from "@/hooks/useSessionContentSearch";
import { useSessionsQuery } from "@/lib/query/queries";
import { sessionsApi } from "@/lib/api/sessions";
import type { SessionMeta } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/lib/api/sessions", () => ({
  sessionsApi: {
    list: vi.fn(),
    syncContentIndex: vi.fn(),
    searchContent: vi.fn(),
  },
}));

function renderSearch(enabled = true) {
  const client = createTestQueryClient();
  return renderHook(
    ({ enabled }) => {
      const list = useSessionsQuery();
      const options = {
        query: "",
        enabled,
        listIsSuccess: list.isSuccess,
        listUpdatedAt: list.dataUpdatedAt,
        providerIds: ["claude"],
      };
      useSessionContentSearch(options);
      return { ...list };
    },
    {
      initialProps: { enabled },
      wrapper: ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    },
  );
}

const session: SessionMeta = {
  providerId: "claude",
  sessionId: "last-session",
  sourcePath: "/test/last-session.jsonl",
};

describe("useSessionContentSearch index synchronization", () => {
  beforeEach(() => {
    vi.mocked(sessionsApi.list).mockReset();
    vi.mocked(sessionsApi.syncContentIndex).mockReset().mockResolvedValue({
      running: false,
      processed: 0,
      total: 0,
    });
  });

  it("waits for a successful list read before synchronizing an empty list", async () => {
    let resolveList!: (sessions: SessionMeta[]) => void;
    vi.mocked(sessionsApi.list).mockReturnValue(
      new Promise((resolve) => {
        resolveList = resolve;
      }),
    );
    const { result } = renderSearch();
    expect(result.current.isPending).toBe(true);
    expect(result.current.dataUpdatedAt).toBe(0);
    expect(sessionsApi.syncContentIndex).not.toHaveBeenCalled();

    await act(async () => resolveList([]));
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    await waitFor(() =>
      expect(sessionsApi.syncContentIndex).toHaveBeenCalledTimes(1),
    );
  });

  it("synchronizes after an external deletion removes the last session", async () => {
    vi.mocked(sessionsApi.list)
      .mockResolvedValueOnce([session])
      .mockResolvedValueOnce([]);
    const { result } = renderSearch();
    await waitFor(() =>
      expect(sessionsApi.syncContentIndex).toHaveBeenCalledTimes(1),
    );

    await act(async () => {
      await result.current.refetch();
    });
    await waitFor(() => expect(result.current.data).toEqual([]));
    await waitFor(() =>
      expect(sessionsApi.syncContentIndex).toHaveBeenCalledTimes(2),
    );
  });

  it("does not synchronize when the initial list read fails", async () => {
    vi.mocked(sessionsApi.list).mockRejectedValue(new Error("scan failed"));
    const { result } = renderSearch();
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.dataUpdatedAt).toBe(0);
    expect(sessionsApi.syncContentIndex).not.toHaveBeenCalled();
  });

  it("does not synchronize again when a list refresh fails", async () => {
    vi.mocked(sessionsApi.list)
      .mockResolvedValueOnce([session])
      .mockRejectedValueOnce(new Error("scan failed"));
    const { result } = renderSearch();
    await waitFor(() =>
      expect(sessionsApi.syncContentIndex).toHaveBeenCalledTimes(1),
    );
    const updatedAt = result.current.dataUpdatedAt;
    await act(async () => {
      await result.current.refetch();
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.dataUpdatedAt).toBe(updatedAt);
    expect(sessionsApi.syncContentIndex).toHaveBeenCalledTimes(1);
  });

  it("waits for recovery when enabled after a failed refresh", async () => {
    vi.mocked(sessionsApi.list)
      .mockResolvedValueOnce([])
      .mockRejectedValueOnce(new Error("scan failed"))
      .mockResolvedValueOnce([]);
    const { result, rerender } = renderSearch(false);
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    const updatedAt = result.current.dataUpdatedAt;
    await act(async () => {
      await result.current.refetch();
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.dataUpdatedAt).toBe(updatedAt);
    expect(updatedAt).toBeGreaterThan(0);
    rerender({ enabled: true });
    expect(sessionsApi.syncContentIndex).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.refetch();
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    await waitFor(() =>
      expect(sessionsApi.syncContentIndex).toHaveBeenCalledTimes(1),
    );
  });

  it("only synchronizes a loaded empty list once search is enabled", async () => {
    vi.mocked(sessionsApi.list).mockResolvedValue([]);
    const { result, rerender } = renderSearch(false);
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(sessionsApi.syncContentIndex).not.toHaveBeenCalled();
    rerender({ enabled: true });
    await waitFor(() =>
      expect(sessionsApi.syncContentIndex).toHaveBeenCalledTimes(1),
    );
  });
});
