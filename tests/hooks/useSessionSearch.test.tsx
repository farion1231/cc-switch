import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import FlexSearch from "flexsearch";
import { useSessionSearch } from "@/hooks/useSessionSearch";
import type { SessionMeta } from "@/types";

const session = (
  index: number,
  overrides: Partial<SessionMeta> = {},
): SessionMeta => ({
  providerId: "claude",
  sessionId: `session-${index}`,
  title: `title ${index}`,
  summary: `summary ${index}`,
  projectDir: `/work/project-${index}`,
  sourcePath: `/home/me/.claude/projects/${index}.jsonl`,
  lastActiveAt: index,
  ...overrides,
});

const ids = (sessions: SessionMeta[]) => sessions.map((s) => s.sessionId);

describe("useSessionSearch", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("returns every session newest first without a query", () => {
    const { result } = renderHook(() =>
      useSessionSearch({
        sessions: [session(1), session(3), session(2)],
        providerFilter: "claude",
      }),
    );

    expect(ids(result.current.search(""))).toEqual([
      "session-3",
      "session-2",
      "session-1",
    ]);
  });

  it("finds a match searched before any background slice has run", () => {
    const sessions = [session(1), session(2, { title: "needle in title" })];
    const { result } = renderHook(() =>
      useSessionSearch({ sessions, providerFilter: "claude" }),
    );

    expect(ids(result.current.search("needle"))).toEqual(["session-2"]);
  });

  it("does not build the index during render", () => {
    const add = vi.spyOn(FlexSearch.Index.prototype, "add");

    renderHook(() =>
      useSessionSearch({
        sessions: [session(1), session(2)],
        providerFilter: "claude",
      }),
    );

    expect(add).not.toHaveBeenCalled();
  });

  it("gives the same results when built in slices and when finished on demand", () => {
    const sessions = Array.from({ length: 50 }, (_, index) =>
      session(index, index % 7 === 0 ? { summary: "shared keyword" } : {}),
    );

    const onDemand = renderHook(() =>
      useSessionSearch({ sessions, providerFilter: "claude" }),
    );
    const direct = ids(onDemand.result.current.search("keyword"));

    const sliced = renderHook(() =>
      useSessionSearch({ sessions, providerFilter: "claude" }),
    );
    act(() => {
      vi.runAllTimers();
    });

    expect(ids(sliced.result.current.search("keyword"))).toEqual(direct);
    expect(direct).toHaveLength(8);
  });

  it("searches every metadata field", () => {
    const sessions = [session(1)];
    const { result } = renderHook(() =>
      useSessionSearch({ sessions, providerFilter: "claude" }),
    );

    for (const query of [
      "session-1",
      "title",
      "summary",
      "project-1",
      "projects",
    ]) {
      expect(ids(result.current.search(query))).toEqual(["session-1"]);
    }
  });

  it("only searches sessions of the selected provider", () => {
    const sessions = [
      session(1, { title: "shared" }),
      session(2, { title: "shared", providerId: "codex" }),
    ];
    const { result, rerender } = renderHook(
      ({ providerFilter }) => useSessionSearch({ sessions, providerFilter }),
      { initialProps: { providerFilter: "claude" } },
    );

    expect(ids(result.current.search("shared"))).toEqual(["session-1"]);

    rerender({ providerFilter: "codex" });
    expect(ids(result.current.search("shared"))).toEqual(["session-2"]);
  });

  it("drops the unfinished background build after unmounting", () => {
    const sessions = Array.from({ length: 5 }, (_, index) => session(index));
    const { unmount } = renderHook(() =>
      useSessionSearch({ sessions, providerFilter: "claude" }),
    );

    unmount();

    expect(vi.getTimerCount()).toBe(0);
  });
});
