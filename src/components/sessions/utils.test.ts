import { describe, expect, it } from "vitest";
import type { SessionMessage, SessionMeta } from "@/types";
import { formatSessionMarkdown, getSessionMarkdownFileName } from "./utils";

describe("formatSessionMarkdown", () => {
  it("exports user and assistant messages in order and preserves Markdown", () => {
    const messages: SessionMessage[] = [
      { role: "system", content: "Internal instructions" },
      { role: "user", content: "Build a greeting." },
      {
        role: "assistant",
        content: 'Here it is:\n\n```ts\nconsole.log("hello");\n```',
      },
      { role: "tool", content: "Command completed" },
      { role: "USER", content: "Thanks" },
    ];

    expect(formatSessionMarkdown(messages)).toBe(
      "## User\n\nBuild a greeting.\n\n" +
        '## Assistant\n\nHere it is:\n\n```ts\nconsole.log("hello");\n```\n\n' +
        "## User\n\nThanks\n",
    );
  });

  it("returns an empty string when there is no conversation content", () => {
    expect(
      formatSessionMarkdown([
        { role: "system", content: "Internal instructions" },
        { role: "assistant", content: "   " },
      ]),
    ).toBe("");
  });
});

describe("getSessionMarkdownFileName", () => {
  it("creates a safe Markdown file name from the session title", () => {
    const session: SessionMeta = {
      providerId: "codex",
      sessionId: "12345678-abcd",
      title: "Fix: auth/login?  ",
    };

    expect(getSessionMarkdownFileName(session)).toBe("Fix- auth-login.md");
  });
});
