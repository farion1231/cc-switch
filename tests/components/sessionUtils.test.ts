import { describe, expect, it } from "vitest";
import {
  buildCdResumeCommand,
  getSessionLastText,
  getSessionTimeBucket,
  groupSessionsByProject,
  groupSessionsByTime,
  isArchivedSession,
  formatMessageTime,
} from "@/components/sessions/utils";
import type { SessionMeta } from "@/types";

describe("session utils", () => {
  it("groups sessions by project directory, newest group first and unknown last", () => {
    const sessions: SessionMeta[] = [
      {
        providerId: "codex",
        sessionId: "unknown",
        projectDir: "  ",
        lastActiveAt: 50,
      },
      {
        providerId: "codex",
        sessionId: "app-new",
        projectDir: "/workspace/app",
        lastActiveAt: 30,
      },
      {
        providerId: "claude",
        sessionId: "docs",
        projectDir: "/workspace/docs",
        lastActiveAt: 40,
      },
      {
        providerId: "claude",
        sessionId: "app-old",
        projectDir: "/workspace/app",
        lastActiveAt: 10,
      },
    ];

    const groups = groupSessionsByProject(sessions, "未知目录");

    expect(groups.map((group) => group.label)).toEqual([
      "docs",
      "app",
      "未知目录",
    ]);
    expect(groups[1].sessions.map((session) => session.sessionId)).toEqual([
      "app-new",
      "app-old",
    ]);
    expect(groups[2].projectDir).toBeNull();
  });

  it("buckets sessions into today / yesterday / this week / earlier", () => {
    // 2026-10-01 is a Thursday; the week starts on Monday 2026-09-28
    const now = new Date(2026, 9, 1, 8, 15).getTime();
    const at = (day: number, hour = 12) =>
      new Date(2026, 8, day, hour).getTime();
    const sessions: SessionMeta[] = [
      { providerId: "claude", sessionId: "a", lastActiveAt: now - 60000 },
      { providerId: "claude", sessionId: "b", lastActiveAt: at(30) },
      { providerId: "claude", sessionId: "c", lastActiveAt: at(28, 9) },
      { providerId: "claude", sessionId: "d", lastActiveAt: at(27) },
    ];

    expect(getSessionTimeBucket(at(28, 0), now)).toBe("thisWeek");
    expect(
      groupSessionsByTime(sessions, now).map((group) => [
        group.bucket,
        group.sessions.map((session) => session.sessionId),
      ]),
    ).toEqual([
      ["today", ["a"]],
      ["yesterday", ["b"]],
      ["thisWeek", ["c"]],
      ["earlier", ["d"]],
    ]);
  });

  it("hides the summary when it repeats the title and marks archived sessions", () => {
    expect(
      getSessionLastText({
        providerId: "gemini",
        sessionId: "g",
        title: "fix: flaky test",
        summary: "fix: flaky test",
      }),
    ).toBe("");
    expect(
      isArchivedSession({
        providerId: "codex",
        sessionId: "x",
        sourcePath: "/Users/me/.codex/archived_sessions/rollout.jsonl",
      }),
    ).toBe(true);
    expect(
      isArchivedSession({
        providerId: "claude",
        sessionId: "c",
        sourcePath: "/Users/me/archived_sessions/a.jsonl",
      }),
    ).toBe(false);
  });

  it("quotes the project directory in the cd-and-resume command", () => {
    expect(buildCdResumeCommand("/tmp/it's", "claude --resume x")).toBe(
      "cd '/tmp/it'\\''s' && claude --resume x",
    );
  });

  it("formats message times with the date, adding the year only for past years", () => {
    const thisYear = new Date().getFullYear();
    const recent = new Date(thisYear, 0, 15, 9, 5).getTime();
    const older = new Date(thisYear - 1, 11, 30, 23, 10).getTime();

    expect(formatMessageTime(undefined)).toBe("");
    // 今年：带月日和时分，不带年份
    expect(formatMessageTime(recent)).toMatch(/15/);
    expect(formatMessageTime(recent)).not.toContain(String(thisYear));
    // 往年：带上年份
    expect(formatMessageTime(older)).toContain(String(thisYear - 1));
    expect(formatMessageTime(older)).toMatch(/30/);
  });
});
