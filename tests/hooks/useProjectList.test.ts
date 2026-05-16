import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { SessionMeta } from "@/types";
import { useProjectList } from "@/hooks/useProjectList";

function makeSession(overrides: Partial<SessionMeta> & { providerId: string; sessionId: string }): SessionMeta {
  return {
    sourcePath: `/fake/${overrides.sessionId}.jsonl`,
    ...overrides,
  };
}

describe("useProjectList", () => {
  it("returns empty array when no sessions have projectDir", () => {
    const sessions = [
      makeSession({ providerId: "claude", sessionId: "s1" }),
      makeSession({ providerId: "claude", sessionId: "s2" }),
    ];

    const { result } = renderHook(() => useProjectList(sessions));
    expect(result.current).toEqual([]);
  });

  it("extracts unique projects with counts", () => {
    const sessions = [
      makeSession({ providerId: "claude", sessionId: "s1", projectDir: "/home/user/proj-a", lastActiveAt: 1000 }),
      makeSession({ providerId: "claude", sessionId: "s2", projectDir: "/home/user/proj-a", lastActiveAt: 2000 }),
      makeSession({ providerId: "claude", sessionId: "s3", projectDir: "/home/user/proj-b", lastActiveAt: 3000 }),
    ];

    const { result } = renderHook(() => useProjectList(sessions));
    const list = result.current;

    expect(list).toHaveLength(2);
    const projA = list.find((p) => p.projectDir === "/home/user/proj-a")!;
    expect(projA.count).toBe(2);
    expect(projA.projectName).toBe("proj-a");
    expect(projA.lastActiveAt).toBe(2000);
  });

  it("sorts by most recent activity (descending) by default", () => {
    const sessions = [
      makeSession({ providerId: "claude", sessionId: "s1", projectDir: "/home/user/alpha", lastActiveAt: 1000 }),
      makeSession({ providerId: "claude", sessionId: "s2", projectDir: "/home/user/zebra", lastActiveAt: 3000 }),
      makeSession({ providerId: "claude", sessionId: "s3", projectDir: "/home/user/middle", lastActiveAt: 2000 }),
    ];

    const { result } = renderHook(() => useProjectList(sessions));
    const names = result.current.map((p) => p.projectName);

    expect(names).toEqual(["zebra", "middle", "alpha"]);
  });

  it("limits to 50 most recent projects", () => {
    const sessions: SessionMeta[] = [];
    for (let i = 0; i < 60; i++) {
      sessions.push(
        makeSession({
          providerId: "claude",
          sessionId: `s${i}`,
          projectDir: `/home/user/proj-${String(i).padStart(3, "0")}`,
          lastActiveAt: i * 1000,
        }),
      );
    }

    const { result } = renderHook(() => useProjectList(sessions));
    const list = result.current;

    expect(list).toHaveLength(50);

    // The 10 oldest should be excluded
    const included = list.map((p) => p.projectName);
    expect(included).not.toContain("proj-000");
    expect(included).toContain("proj-010");
    expect(included).toContain("proj-059");

    // Sorted by most recent first
    expect(included[0]).toBe("proj-059");
  });

  it("uses the most recent lastActiveAt across sessions in the same project", () => {
    const sessions = [
      makeSession({ providerId: "claude", sessionId: "s1", projectDir: "/proj", lastActiveAt: 1000 }),
      makeSession({ providerId: "claude", sessionId: "s2", projectDir: "/proj", lastActiveAt: 5000 }),
      makeSession({ providerId: "claude", sessionId: "s3", projectDir: "/proj", lastActiveAt: 3000 }),
    ];

    const { result } = renderHook(() => useProjectList(sessions));
    expect(result.current[0].lastActiveAt).toBe(5000);
  });

  it("extracts basename from Windows paths", () => {
    const sessions = [
      makeSession({ providerId: "claude", sessionId: "s1", projectDir: "D:\\Projects\\my-app", lastActiveAt: 1000 }),
    ];

    const { result } = renderHook(() => useProjectList(sessions));
    expect(result.current[0].projectName).toBe("my-app");
  });

  it("filters by providerFilter when provided", () => {
    const sessions = [
      makeSession({ providerId: "claude", sessionId: "s1", projectDir: "/proj-a", lastActiveAt: 1000 }),
      makeSession({ providerId: "codex", sessionId: "s2", projectDir: "/proj-b", lastActiveAt: 2000 }),
      makeSession({ providerId: "codex", sessionId: "s3", projectDir: "/proj-a", lastActiveAt: 3000 }),
    ];

    const { result } = renderHook(() => useProjectList(sessions, "codex"));
    const list = result.current;

    expect(list).toHaveLength(2);
    const projA = list.find((p) => p.projectDir === "/proj-a")!;
    expect(projA.count).toBe(1);
  });

  it("returns all projects when providerFilter is 'all'", () => {
    const sessions = [
      makeSession({ providerId: "claude", sessionId: "s1", projectDir: "/proj-a", lastActiveAt: 1000 }),
      makeSession({ providerId: "codex", sessionId: "s2", projectDir: "/proj-b", lastActiveAt: 2000 }),
    ];

    const { result } = renderHook(() => useProjectList(sessions, "all"));
    expect(result.current).toHaveLength(2);
  });
});
