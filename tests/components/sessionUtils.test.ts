import { describe, expect, it } from "vitest";
import {
  buildCdResumeCommand,
  buildSessionDisplayItems,
  extractCodexPromptPreview,
  formatSessionMessagePreview,
  formatToolNames,
  getSessionLastText,
  getSessionTimeBucket,
  groupSessionsByProject,
  groupSessionsByTime,
  isArchivedSession,
  isChatItem,
  shouldHideCodexMessageFromToc,
  splitInlineCode,
  splitMarkdownBlocks,
} from "@/components/sessions/utils";
import type { SessionMeta } from "@/types";

describe("session utils", () => {
  it("extracts Codex VS Code prompts after the request marker", () => {
    const content = [
      "# Context from my IDE setup:",
      "",
      "## Active file: src/main.ts",
      "",
      "## My request for Codex:",
      "Fix the session title preview",
    ].join("\n");

    expect(extractCodexPromptPreview(content)).toBe(
      "Fix the session title preview",
    );
  });

  it("extracts inline Codex VS Code prompts", () => {
    const content = [
      "# Context from my IDE setup:",
      "",
      "## My request for Codex: Fix the TOC preview",
    ].join("\n");

    expect(extractCodexPromptPreview(content)).toBe("Fix the TOC preview");
  });

  it("ignores marker mentions before the Codex request heading", () => {
    const content = [
      "# Context from my IDE setup:",
      "",
      "## Active selection:",
      "My request for Codex: not the prompt",
      "",
      "## My request for Codex:",
      "Use the real request heading",
    ].join("\n");

    expect(extractCodexPromptPreview(content)).toBe(
      "Use the real request heading",
    );
  });

  it("uses the last request heading when the selection contains one", () => {
    const content = [
      "# Context from my IDE setup:",
      "",
      "## Active selection: docs/codex-format.md:10-14",
      "## My request for Codex:",
      "selected document content, not the real request",
      "",
      "## My request for Codex:",
      "the real injected request",
    ].join("\n");

    expect(extractCodexPromptPreview(content)).toBe(
      "the real injected request",
    );
  });

  // Known limitation: the IDE marker is matched purely by text, so a
  // "## My request for Codex:" line inside the real request body is treated as
  // a new boundary and only the trailing part is kept. Pinning this documents
  // the best-effort behavior; fully fixing it needs structured IDE section data
  // that the Codex VS Code context does not provide.
  it("keeps only the trailing part when the request body repeats the heading", () => {
    const content = [
      "# Context from my IDE setup:",
      "",
      "## Active file: foo.ts",
      "",
      "## My request for Codex:",
      "Document the format, for example:",
      "## My request for Codex:",
      "and the rest follows.",
    ].join("\n");

    expect(extractCodexPromptPreview(content)).toBe("and the rest follows.");
  });

  it("does not extract from ordinary messages that mention the marker", () => {
    const content = "Please explain the phrase My request for Codex.";

    expect(extractCodexPromptPreview(content)).toBe(content);
  });

  it("hides Codex context messages without user prompts from the TOC", () => {
    expect(
      shouldHideCodexMessageFromToc("# AGENTS.md instructions for F:/project"),
    ).toBe(true);
    expect(
      shouldHideCodexMessageFromToc(
        "<environment_context>\n<cwd>F:/project</cwd>",
      ),
    ).toBe(true);
    expect(shouldHideCodexMessageFromToc("# Context from my IDE setup:")).toBe(
      true,
    );
    expect(
      shouldHideCodexMessageFromToc(
        "# Context from my IDE setup:\n\n## My request for Codex:\nFix it",
      ),
    ).toBe(false);
  });

  it("formats message previews with truncation", () => {
    expect(formatSessionMessagePreview("short message")).toBe("short message");
    expect(formatSessionMessagePreview("a".repeat(51))).toBe(
      `${"a".repeat(50)}...`,
    );
  });

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

  it("merges consecutive tool calls and outputs into one display item", () => {
    const items = buildSessionDisplayItems([
      { role: "user", content: "look at the layout" },
      { role: "assistant", content: "Sure.\n[Tool: Read]" },
      { role: "tool", content: "file contents" },
      { role: "assistant", content: "[Tool: Grep]" },
      { role: "assistant", content: "[Tool: Grep]" },
      { role: "tool", content: "matches" },
      { role: "assistant", content: "Done." },
    ]);

    expect(items.map((item) => item.kind)).toEqual([
      "message",
      "message",
      "tools",
      "message",
    ]);
    const tools = items[2];
    expect(tools.kind === "tools" && tools.names).toEqual([
      "Read",
      "Grep",
      "Grep",
    ]);
    expect(tools.kind === "tools" && tools.output).toBe(
      "file contents\nmatches",
    );
    expect(items[1].kind === "message" && items[1].content).toBe("Sure.");
    expect(formatToolNames(["Read", "Grep", "Grep", "Grep", "Edit"])).toBe(
      "Read · Grep ×3 · Edit",
    );
    expect(isChatItem(tools)).toBe(false);
  });

  it("splits fenced code blocks and inline code", () => {
    expect(splitMarkdownBlocks("Before\n```tsx\n<div />\n```\nAfter")).toEqual([
      { type: "text", text: "Before" },
      { type: "code", lang: "tsx", code: "<div />" },
      { type: "text", text: "After" },
    ]);
    expect(splitMarkdownBlocks("```\nunclosed")).toEqual([
      { type: "code", lang: "", code: "unclosed" },
    ]);
    expect(splitInlineCode("use `SessionMeta` here")).toEqual([
      { code: false, text: "use " },
      { code: true, text: "SessionMeta" },
      { code: false, text: " here" },
    ]);
  });

  it("quotes the project directory in the cd-and-resume command", () => {
    expect(buildCdResumeCommand("/tmp/it's", "claude --resume x")).toBe(
      "cd '/tmp/it'\\''s' && claude --resume x",
    );
  });
});
