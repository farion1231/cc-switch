import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { SessionMeta } from "@/types";
import { useSessionSearch } from "@/hooks/useSessionSearch";

function makeSession(overrides: Partial<SessionMeta> & { providerId: string; sessionId: string }): SessionMeta {
  return {
    sourcePath: `/fake/${overrides.sessionId}.jsonl`,
    lastActiveAt: 1000,
    ...overrides,
  };
}

describe("useSessionSearch with projectFilter", () => {
  const sessions: SessionMeta[] = [
    makeSession({ providerId: "claude", sessionId: "s1", projectDir: "/proj-a", title: "Fix bug" }),
    makeSession({ providerId: "claude", sessionId: "s2", projectDir: "/proj-a", title: "Add feature" }),
    makeSession({ providerId: "codex", sessionId: "s3", projectDir: "/proj-b", title: "Refactor" }),
    makeSession({ providerId: "claude", sessionId: "s4", title: "No project session" }),
  ];

  it("returns all sessions when projectFilter is null", () => {
    const { result } = renderHook(() =>
      useSessionSearch({ sessions, providerFilter: "all", projectFilter: null }),
    );

    const found = result.current.search("");
    expect(found).toHaveLength(4);
  });

  it("filters by projectDir when projectFilter is set", () => {
    const { result } = renderHook(() =>
      useSessionSearch({ sessions, providerFilter: "all", projectFilter: "/proj-a" }),
    );

    const found = result.current.search("");
    expect(found).toHaveLength(2);
    expect(found.every((s) => s.projectDir === "/proj-a")).toBe(true);
  });

  it("hides sessions without projectDir when projectFilter is set", () => {
    const { result } = renderHook(() =>
      useSessionSearch({ sessions, providerFilter: "all", projectFilter: "/proj-a" }),
    );

    const found = result.current.search("");
    const noProject = found.find((s) => !s.projectDir);
    expect(noProject).toBeUndefined();
  });

  it("combines projectFilter and providerFilter", () => {
    const { result } = renderHook(() =>
      useSessionSearch({ sessions, providerFilter: "codex", projectFilter: "/proj-b" }),
    );

    const found = result.current.search("");
    expect(found).toHaveLength(1);
    expect(found[0].sessionId).toBe("s3");
  });

  it("returns empty when projectFilter and providerFilter have no overlap", () => {
    const { result } = renderHook(() =>
      useSessionSearch({ sessions, providerFilter: "codex", projectFilter: "/proj-a" }),
    );

    const found = result.current.search("");
    expect(found).toHaveLength(0);
  });

  it("search query works within projectFilter", () => {
    const { result } = renderHook(() =>
      useSessionSearch({ sessions, providerFilter: "all", projectFilter: "/proj-a" }),
    );

    const found = result.current.search("bug");
    expect(found).toHaveLength(1);
    expect(found[0].title).toBe("Fix bug");
  });
});
