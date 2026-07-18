import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { providerPresets } from "@/config/claudeProviderPresets";

describe("xAI OAuth provider presets", () => {
  it("pins the Claude Code preset to managed Responses auth", () => {
    const preset = providerPresets.find((entry) => entry.name === "xAI (Grok)");
    expect(preset).toBeDefined();
    expect(preset).toMatchObject({
      category: "third_party",
      apiFormat: "openai_responses",
      providerType: "xai_oauth",
      requiresOAuth: true,
      icon: "xai",
    });
    expect((preset!.settingsConfig as any).env).toMatchObject({
      ANTHROPIC_BASE_URL: "https://api.x.ai/v1",
      ANTHROPIC_MODEL: "grok-4.5",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "grok-4.5",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "grok-4.5",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "grok-4.5",
    });
    expect((preset!.settingsConfig as any).env).not.toHaveProperty(
      "ANTHROPIC_API_KEY",
    );
    expect((preset!.settingsConfig as any).env).not.toHaveProperty(
      "ANTHROPIC_AUTH_TOKEN",
    );
  });

  it("pins the Claude Desktop preset to proxy Responses mode without 1M", () => {
    const preset = claudeDesktopProviderPresets.find(
      (entry) => entry.name === "xAI (Grok)",
    );
    expect(preset).toMatchObject({
      category: "third_party",
      baseUrl: "https://api.x.ai/v1",
      mode: "proxy",
      apiFormat: "openai_responses",
      providerType: "xai_oauth",
      requiresOAuth: true,
      icon: "xai",
    });
    expect(preset!.modelRoutes).toEqual([
      expect.objectContaining({
        upstreamModel: "grok-4.5",
        supports1m: false,
      }),
    ]);
  });
});
