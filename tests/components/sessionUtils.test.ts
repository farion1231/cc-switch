import { describe, expect, it } from "vitest";
import {
  extractCodexPromptPreview,
  formatSessionMessagePreview,
  shouldHideCodexMessageFromToc,
} from "@/components/sessions/utils";

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
});
