import { renderHook, act, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { useSessionContentSearch } from "@/hooks/useSessionSearch";
import type { SessionSearchHit } from "@/types";

const searchMock = vi.fn();

vi.mock("@/lib/api", () => ({
  sessionsApi: {
    search: (...args: unknown[]) => searchMock(...args),
  },
}));

const hit = (sourcePath: string): SessionSearchHit => ({
  providerId: "claude",
  sessionId: sourcePath,
  sourcePath,
  snippets: [{ messageIndex: 2, role: "assistant", text: "…匹配片段…" }],
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

const render = (query: string, providerFilter = "all") =>
  renderHook(({ q, filter }) => useSessionContentSearch(q, filter), {
    initialProps: { q: query, filter: providerFilter },
  });

describe("useSessionContentSearch", () => {
  beforeEach(() => {
    searchMock.mockReset();
  });

  it("exposes backend snippets keyed by source path", async () => {
    searchMock.mockResolvedValue([hit("/a.jsonl")]);

    const { result } = render("浙江移动");

    await waitFor(() =>
      expect([...result.current.snippetsBySource.keys()]).toEqual(["/a.jsonl"]),
    );
    expect(result.current.snippetsBySource.get("/a.jsonl")).toHaveLength(1);
  });

  it("drops snippets from the previous query while the new one is in flight", async () => {
    searchMock.mockResolvedValueOnce([hit("/a.jsonl")]);
    const { result, rerender } = render("alpha");

    await waitFor(() => expect(result.current.snippetsBySource.size).toBe(1));

    const pending = deferred<SessionSearchHit[]>();
    searchMock.mockImplementationOnce(() => pending.promise);
    rerender({ q: "beta", filter: "all" });

    // "alpha" 的命中必须立刻消失，否则新关键词下会显示上一次的片段
    expect(result.current.snippetsBySource.size).toBe(0);

    await act(async () => {
      pending.resolve([hit("/b.jsonl")]);
      await pending.promise;
    });

    await waitFor(() =>
      expect([...result.current.snippetsBySource.keys()]).toEqual(["/b.jsonl"]),
    );
  });

  it("drops snippets from the previous provider filter", async () => {
    searchMock.mockResolvedValueOnce([hit("/a.jsonl")]);
    const { result, rerender } = render("alpha", "claude");

    await waitFor(() => expect(result.current.snippetsBySource.size).toBe(1));

    searchMock.mockImplementationOnce(
      () => deferred<SessionSearchHit[]>().promise,
    );
    rerender({ q: "alpha", filter: "codex" });

    expect(result.current.snippetsBySource.size).toBe(0);
  });

  it("ignores a late reply from a superseded query", async () => {
    const stale = deferred<SessionSearchHit[]>();
    searchMock.mockImplementationOnce(() => stale.promise);
    const { result, rerender } = render("alpha");

    await waitFor(() => expect(searchMock).toHaveBeenCalledTimes(1));

    searchMock.mockResolvedValueOnce([hit("/b.jsonl")]);
    rerender({ q: "beta", filter: "all" });
    await waitFor(() =>
      expect([...result.current.snippetsBySource.keys()]).toEqual(["/b.jsonl"]),
    );

    await act(async () => {
      stale.resolve([hit("/a.jsonl")]);
      await stale.promise;
    });

    expect([...result.current.snippetsBySource.keys()]).toEqual(["/b.jsonl"]);
  });

  it("searches every provider when the list is unfiltered", async () => {
    searchMock.mockResolvedValue([]);

    render("alpha");
    await waitFor(() =>
      expect(searchMock).toHaveBeenCalledWith(
        "alpha",
        undefined,
        expect.any(Number),
      ),
    );

    searchMock.mockClear();
    render("alpha", "codex");
    await waitFor(() =>
      expect(searchMock).toHaveBeenCalledWith(
        "alpha",
        "codex",
        expect.any(Number),
      ),
    );
  });

  // 后端靠这个递增号丢弃已被新关键词取代的扫描；号不递增就等于没有取消机制
  it("tags each scan with an increasing request id", async () => {
    searchMock.mockResolvedValue([]);
    const { rerender } = render("alpha");

    await waitFor(() => expect(searchMock).toHaveBeenCalledTimes(1));
    rerender({ q: "beta", filter: "all" });
    await waitFor(() => expect(searchMock).toHaveBeenCalledTimes(2));

    const [first, second] = searchMock.mock.calls;
    expect(second[2]).toBeGreaterThan(first[2]);
  });

  it("skips the backend for a blank query", async () => {
    const { result } = render("   ");

    await new Promise((resolve) => setTimeout(resolve, 400));
    expect(searchMock).not.toHaveBeenCalled();
    expect(result.current.snippetsBySource.size).toBe(0);
    expect(result.current.isSearching).toBe(false);
  });
});
