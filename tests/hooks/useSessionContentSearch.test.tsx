import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSessionContentSearch } from "@/hooks/useSessionContentSearch";
import { useSessionsQuery } from "@/lib/query/queries";
import { sessionsApi } from "@/lib/api/sessions";
import type { SessionContentSearchResult, SessionMeta } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/lib/api/sessions", () => ({
  sessionsApi: {
    list: vi.fn(),
    syncContentIndex: vi.fn(),
    searchContent: vi.fn(),
  },
}));

function renderSearch(
  enabled = true,
  options: Partial<{ query: string; providerIds: string[] }> = {},
) {
  const client = createTestQueryClient();
  return renderHook(
    ({ enabled }: { enabled: boolean }) => {
      const list = useSessionsQuery();
      const content = useSessionContentSearch({
        query: options.query ?? "",
        enabled,
        listIsSuccess: list.isSuccess,
        listUpdatedAt: list.dataUpdatedAt,
        providerIds: options.providerIds ?? ["claude"],
      });
      return { ...list, content };
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

describe("useSessionContentSearch failure reporting", () => {
  beforeEach(() => {
    vi.mocked(sessionsApi.list).mockReset().mockResolvedValue([session]);
    vi.mocked(sessionsApi.syncContentIndex)
      .mockReset()
      .mockResolvedValue({ running: false, processed: 0, total: 0 });
    vi.mocked(sessionsApi.searchContent).mockReset();
  });

  it("exposes a failed content query instead of looking like no matches", async () => {
    vi.mocked(sessionsApi.searchContent).mockRejectedValue(
      "Session index error: database disk image is malformed",
    );
    const { result } = renderSearch(true, { query: "needle" });

    await waitFor(() =>
      expect(result.current.content.error).toContain("malformed"),
    );
    // 失败时空命中不代表正文里没有这个词，界面据此区别于「没搜到」
    expect(result.current.content.hits.size).toBe(0);
  });

  it("reports no error for an ordinary empty result", async () => {
    vi.mocked(sessionsApi.searchContent).mockResolvedValue({
      hits: [],
      status: { running: false, processed: 0, total: 0 },
    });
    const { result } = renderSearch(true, { query: "needle" });

    await waitFor(() => expect(sessionsApi.searchContent).toHaveBeenCalled());
    expect(result.current.content.error).toBeNull();
  });

  it("does not report a failure while content search is off", async () => {
    vi.mocked(sessionsApi.searchContent).mockRejectedValue("boom");
    const { result } = renderSearch(false, { query: "needle" });

    // 关闭时根本不查，界面也不该出现失败提示
    expect(result.current.content.error).toBeNull();
    expect(sessionsApi.searchContent).not.toHaveBeenCalled();
  });

  it("clears the failure once a retry succeeds", async () => {
    vi.mocked(sessionsApi.searchContent)
      .mockRejectedValueOnce(
        "Session index error: database disk image is malformed",
      )
      .mockResolvedValueOnce({
        hits: [],
        status: { running: false, processed: 0, total: 0 },
      });
    const { result } = renderSearch(true, { query: "needle" });
    await waitFor(() =>
      expect(result.current.content.error).toContain("malformed"),
    );

    await act(async () => {
      result.current.content.retry();
    });
    // 重试真的重新查了一次，成功后提示消失
    await waitFor(() => expect(result.current.content.error).toBeNull());
    expect(sessionsApi.searchContent).toHaveBeenCalledTimes(2);
  });

  it("re-syncs the index on retry so a rebuilt-but-empty index gets refilled", async () => {
    // 重试时后端会删掉坏索引重建：建出来是空的，只重新查询会「成功返回零命中」，
    // 界面又回到「没有会话包含该词」。所以重试必须同时触发一次重新收录。
    vi.mocked(sessionsApi.searchContent)
      .mockResolvedValue({
        hits: [],
        status: { running: false, processed: 0, total: 0 },
      })
      .mockRejectedValueOnce(
        "Session index error: database disk image is malformed",
      );
    const { result } = renderSearch(true, { query: "needle" });
    await waitFor(() =>
      expect(result.current.content.error).toContain("malformed"),
    );
    const syncBefore = vi.mocked(sessionsApi.syncContentIndex).mock.calls
      .length;

    await act(async () => {
      result.current.content.retry();
    });
    await waitFor(() =>
      expect(sessionsApi.syncContentIndex).toHaveBeenCalledTimes(
        syncBefore + 1,
      ),
    );
  });

  it("keeps a non-empty reason even when the command rejects with an empty message", async () => {
    vi.mocked(sessionsApi.searchContent).mockRejectedValue("");
    const { result } = renderSearch(true, { query: "needle" });

    // 空原因会让界面退回「没有匹配」，所以至少要给出非空文本
    await waitFor(() => expect(result.current.content.error).not.toBeNull());
    expect(result.current.content.error).not.toBe("");
  });
});

describe("useSessionContentSearch cancellation", () => {
  const emptyResult: SessionContentSearchResult = {
    hits: [],
    status: { running: false, processed: 0, total: 0 },
  };

  function renderCancellableSearch() {
    const requests: {
      query: string;
      signal: AbortSignal;
      resolve: (result: SessionContentSearchResult) => void;
      reject: (error: unknown) => void;
    }[] = [];
    vi.mocked(sessionsApi.searchContent)
      .mockReset()
      .mockImplementation(
        (query, _providers, signal) =>
          new Promise((resolve, reject) => {
            requests.push({ query, signal: signal!, resolve, reject });
          }),
      );
    const client = createTestQueryClient();
    const hook = renderHook(
      (props: { query: string; enabled: boolean; providerIds: string[] }) =>
        useSessionContentSearch({
          ...props,
          listIsSuccess: false,
          listUpdatedAt: 0,
        }),
      {
        initialProps: { query: "的", enabled: true, providerIds: ["codex"] },
        wrapper: ({ children }: { children: ReactNode }) => (
          <QueryClientProvider client={client}>{children}</QueryClientProvider>
        ),
      },
    );
    return { ...hook, requests };
  }

  it("aborts on input changes before debounce and ignores an obsolete failure", async () => {
    const { requests, rerender, result, unmount } = renderCancellableSearch();
    expect(requests).toHaveLength(1);
    rerender({ query: "的会", enabled: true, providerIds: ["codex"] });
    expect(requests[0].signal.aborted).toBe(true);
    expect(requests).toHaveLength(1);
    rerender({ query: "的会话", enabled: true, providerIds: ["codex"] });
    await act(async () =>
      requests[0].reject("Session content search cancelled"),
    );
    expect(result.current.error).toBeNull();
    await waitFor(() => expect(requests).toHaveLength(2));
    expect(requests[1].query).toBe("的会话");
    expect(requests[1].signal.aborted).toBe(false);
    unmount();
    expect(requests[1].signal.aborted).toBe(true);
  });

  it("aborts when disabled and starts a fresh request when re-enabled", async () => {
    const { requests, rerender, result, unmount } = renderCancellableSearch();
    rerender({ query: "的", enabled: false, providerIds: ["codex"] });
    expect(requests[0].signal.aborted).toBe(true);
    expect(result.current.error).toBeNull();
    rerender({ query: "的", enabled: true, providerIds: ["codex"] });
    await waitFor(() => expect(requests).toHaveLength(2));
    expect(requests[1].signal.aborted).toBe(false);
    unmount();
    expect(requests[1].signal.aborted).toBe(true);
  });

  it("aborts when filters change or the input is cleared without showing stale hits", async () => {
    const { requests, rerender, result } = renderCancellableSearch();
    rerender({ query: "的", enabled: true, providerIds: ["claude"] });
    expect(requests[0].signal.aborted).toBe(true);
    await waitFor(() => expect(requests).toHaveLength(2));
    rerender({ query: "", enabled: true, providerIds: ["claude"] });
    expect(requests[1].signal.aborted).toBe(true);
    await act(async () => {
      requests[0].resolve({
        ...emptyResult,
        hits: [
          {
            providerId: "codex",
            sourcePath: "/stale",
            snippets: [],
            matchCount: 1,
          },
        ],
      });
      requests[1].resolve(emptyResult);
    });
    expect(requests).toHaveLength(2);
    expect(result.current.hits.size).toBe(0);
    expect(result.current.error).toBeNull();
  });
});
