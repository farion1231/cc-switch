import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { searchTerms, useSessionSearch } from "@/hooks/useSessionSearch";
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

const searchOne = (overrides: Partial<SessionMeta>, query: string) => {
  const { result } = renderHook(() =>
    useSessionSearch({
      sessions: [session(1, overrides)],
      providerFilter: "claude",
    }),
  );
  return ids(result.current.search(query));
};

describe("useSessionSearch", () => {
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

  it("searches every metadata field", () => {
    for (const query of [
      "session-1",
      "title",
      "summary",
      "project-1",
      "projects",
    ]) {
      expect(searchOne({}, query)).toEqual(["session-1"]);
    }
  });

  it("matches substrings of words regardless of case and accents", () => {
    const overrides = { title: "Café Refactor", summary: "会话列表性能" };
    for (const query of ["CAFE", "café", "fact", "话列", "性能"]) {
      expect(searchOne(overrides, query)).toEqual(["session-1"]);
    }
  });

  it("requires every query word and ignores punctuation between them", () => {
    const overrides = { title: "fix streaming proxy" };
    expect(searchOne(overrides, "proxy fix")).toEqual(["session-1"]);
    expect(searchOne(overrides, "fix-stream")).toEqual(["session-1"]);
    expect(searchOne(overrides, "proxy missing")).toEqual([]);
    // Windows paths and slashes split into words like any other separator
    expect(
      searchOne({ projectDir: "D:\\Projects\\cc-switch" }, "d:/projects"),
    ).toEqual(["session-1"]);
  });

  it("does not match across separate words", () => {
    expect(searchOne({ title: "pro xy" }, "proxy")).toEqual([]);
  });

  it("keeps collapsing repeated letters like the previous index", () => {
    expect(searchOne({ title: "hello" }, "helo")).toEqual(["session-1"]);
    expect(searchOne({ title: "helo" }, "hello")).toEqual(["session-1"]);
  });

  it("matches digits as plain substrings", () => {
    const overrides = { sourcePath: "/sessions/rollout-2026-10-01.jsonl" };
    expect(searchOne(overrides, "026")).toEqual(["session-1"]);
    expect(searchOne(overrides, "rollout 2026")).toEqual(["session-1"]);
  });

  it("returns nothing for a query made only of separators", () => {
    expect(searchOne({}, "--- /")).toEqual([]);
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

    rerender({ providerFilter: "all" });
    expect(ids(result.current.search("shared"))).toEqual([
      "session-1",
      "session-2",
    ]);
  });

  it("follows changed session data", () => {
    const { result, rerender } = renderHook(
      ({ sessions }) =>
        useSessionSearch({ sessions, providerFilter: "claude" }),
      { initialProps: { sessions: [session(1, { title: "oldname" })] } },
    );
    expect(ids(result.current.search("oldname"))).toEqual(["session-1"]);

    rerender({ sessions: [session(2, { title: "newname" })] });
    expect(result.current.search("oldname")).toEqual([]);
    expect(ids(result.current.search("newname"))).toEqual(["session-2"]);
  });
});

describe("searchTerms", () => {
  it("normalizes and splits text into words", () => {
    expect(searchTerms("Café-Über  会话/Path_2026")).toEqual([
      "cafe",
      "uber",
      "会话",
      "path",
      "2026",
    ]);
  });
});
