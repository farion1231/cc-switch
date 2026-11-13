import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";

describe("Claude Desktop OpenCode Go 预设", () => {
  const preset = claudeDesktopProviderPresets.find(
    (item) => item.name === "OpenCode Go",
  );

  it("默认走官方文档列出的 /inference/go/anthropic", () => {
    expect(preset?.baseUrl).toBe("https://opencode.ai/inference/go/anthropic");
  });
});
