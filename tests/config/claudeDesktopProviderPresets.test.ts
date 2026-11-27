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

describe("Claude Desktop Command Code 预设", () => {
  const preset = claudeDesktopProviderPresets.find(
    (item) => item.name === "Command Code",
  );

  it("与 Claude Code 预设一致，经路由按 OpenAI Chat 转发", () => {
    expect(preset?.baseUrl).toBe("https://api.commandcode.ai/provider");
    expect(preset?.mode).toBe("proxy");
    expect(preset?.apiFormat).toBe("openai_chat");
    expect(preset?.modelRoutes?.[0]?.upstreamModel).toBe(
      "deepseek/deepseek-v4.1-flash",
    );
  });
});
