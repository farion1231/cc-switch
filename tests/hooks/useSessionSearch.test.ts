import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useSessionSearch } from "@/hooks/useSessionSearch";
import type { SessionMeta } from "@/types";

const sessions: SessionMeta[] = [
  {
    providerId: "codex",
    sessionId: "550e8400-e29b-41d4-a716-446655440000",
    title: "代理路由 Debug",
    summary: "Fix ÉCLAIR handling",
    projectDir: "/work/My_Project",
    sourcePath: "/sessions/2026-11-21/run.jsonl",
    lastActiveAt: 1,
  },
  {
    providerId: "claude",
    sessionId: "second",
    title: "Debug",
    lastActiveAt: 2,
  },
];

describe("useSessionSearch", () => {
  it("matches literal substrings and requires every term, including across fields", () => {
    const { result } = renderHook(() =>
      useSessionSearch({ sessions, providerFilter: "all" }),
    );
    for (const query of [
      "路由",
      "DEBUG éclair",
      "E29B-41D4 my_project",
      "21/run.",
    ]) {
      expect(result.current.search(query)).toEqual([sessions[0]]);
    }
    expect(result.current.search("debug missing")).toEqual([]);
    expect(result.current.search("_")).toEqual([sessions[0]]);
    expect(result.current.search("%*")).toEqual([]);
    // Sorting belongs to the page, including for an empty query.
    expect(result.current.search(" \n ")).toEqual(sessions);
  });

  it("prepares metadata lazily and reuses it across application filters", () => {
    const title = vi.fn(() => "代理 Debug");
    const data = [
      {
        ...sessions[0],
        get title() {
          return title();
        },
      },
      sessions[1],
    ];
    const { result, rerender } = renderHook(
      ({ providerFilter, providerIds, sessions }) =>
        useSessionSearch({ providerFilter, providerIds, sessions }),
      {
        initialProps: {
          providerFilter: "all",
          providerIds: ["codex", "claude"],
          sessions: data,
        },
      },
    );
    expect(result.current.search("")).toHaveLength(2);
    expect(title).not.toHaveBeenCalled();
    expect(result.current.search("debug")).toHaveLength(2);
    expect(title).toHaveBeenCalledTimes(1);
    rerender({
      providerFilter: "claude",
      providerIds: ["codex", "claude"],
      sessions: data,
    });
    expect(result.current.search("debug")).toEqual([sessions[1]]);
    rerender({ providerFilter: "all", providerIds: ["codex"], sessions: data });
    expect(result.current.search("debug")).toHaveLength(1);
    expect(title).toHaveBeenCalledTimes(1);
    const next = [{ ...sessions[0], title: "changed" }];
    rerender({ providerFilter: "all", providerIds: ["codex"], sessions: next });
    expect(result.current.search("changed")).toEqual(next);
    expect(result.current.search("debug")).toEqual([]);
  });
});
