import { describe, expect, it } from "vitest";
import {
  extractCodexPromptPreview,
  isCodexInjectedContext,
} from "@/components/sessions/utils";

describe("extractCodexPromptPreview", () => {
  it("returns only the prompt after the Codex heading", () => {
    const content = [
      "# Context from my IDE setup:",
      "",
      "## Active file: vendor/clink.lua",
      "",
      "## Open tabs:",
      "- clink.lua: vendor/clink.lua",
      "",
      "## My request for Codex:",
      "请帮我修复这里的逻辑",
    ].join("\n");

    expect(extractCodexPromptPreview(content)).toBe("请帮我修复这里的逻辑");
  });

  it("supports inline Codex heading text", () => {
    const content = [
      "# Context from my IDE setup:",
      "",
      "## My request for Codex: 请帮我修复这里的逻辑",
    ].join("\n");

    expect(extractCodexPromptPreview(content)).toBe("请帮我修复这里的逻辑");
  });

  it("leaves non-Codex content unchanged", () => {
    expect(extractCodexPromptPreview("普通对话内容")).toBe("普通对话内容");
  });

  it("detects injected Codex context messages", () => {
    expect(
      isCodexInjectedContext(
        "# AGENTS.md instructions for d:\\sail\\project\n\n<INSTRUCTIONS>",
      ),
    ).toBe(true);
    expect(
      isCodexInjectedContext(
        "<environment_context>\n  <cwd>d:\\sail\\project</cwd>\n</environment_context>",
      ),
    ).toBe(true);
    expect(isCodexInjectedContext("请你先熟悉一下当前的项目")).toBe(false);
  });
});
