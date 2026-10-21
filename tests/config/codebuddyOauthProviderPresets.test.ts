import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";

describe("CodeBuddy OAuth provider presets", () => {
  it("pins the Claude Code preset to managed OpenAI Chat auth", () => {
    const preset = providerPresets.find((entry) => entry.name === "CodeBuddy");
    expect(preset).toBeDefined();
    expect(preset).toMatchObject({
      websiteUrl: "https://copilot.tencent.com",
      category: "third_party",
      apiFormat: "openai_chat",
      providerType: "codebuddy_oauth",
      requiresOAuth: true,
      icon: "codebuddy",
      iconColor: "#6C4DFF",
    });
    expect((preset!.settingsConfig as any).env).toEqual({
      // The proxy pins this origin and the Chat Completions path; requests are
      // forced to stream and the CLI headers are injected per account.
      ANTHROPIC_BASE_URL: "https://copilot.tencent.com",
      ANTHROPIC_MODEL: "glm-5.3",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "glm-5.3",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.3",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "glm-5.3",
      CLAUDE_CODE_MAX_CONTEXT_TOKENS: "200000",
    });
    expect((preset!.settingsConfig as any).env).not.toHaveProperty(
      "ANTHROPIC_API_KEY",
    );
    expect((preset!.settingsConfig as any).env).not.toHaveProperty(
      "ANTHROPIC_AUTH_TOKEN",
    );
  });

  it("pins the Claude Desktop preset to proxy Chat mode without 1M", () => {
    const preset = claudeDesktopProviderPresets.find(
      (entry) => entry.name === "CodeBuddy",
    );
    expect(preset).toMatchObject({
      websiteUrl: "https://copilot.tencent.com",
      category: "third_party",
      baseUrl: "https://copilot.tencent.com",
      mode: "proxy",
      apiFormat: "openai_chat",
      providerType: "codebuddy_oauth",
      requiresOAuth: true,
      icon: "codebuddy",
      iconColor: "#6C4DFF",
    });
    expect(preset!.modelRoutes).toEqual([
      expect.objectContaining({
        upstreamModel: "glm-5.3",
        supports1m: false,
      }),
    ]);
  });

  it("has no Codex preset", () => {
    expect(
      codexProviderPresets.some((entry) => entry.name === "CodeBuddy"),
    ).toBe(false);
  });
});
