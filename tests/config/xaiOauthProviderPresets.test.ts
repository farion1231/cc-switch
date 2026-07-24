import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import {
  extractCodexBaseUrl,
  extractCodexModelName,
  extractCodexWireApi,
} from "@/utils/providerConfigUtils";

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

  it("pins the Codex preset to native Responses via API key (no managed OAuth)", () => {
    const preset = codexProviderPresets.find(
      (entry) => entry.name === "xAI (Grok)",
    );
    expect(preset).toBeDefined();
    expect(preset).toMatchObject({
      category: "third_party",
      apiFormat: "openai_responses",
      icon: "xai",
    });
    // API-key preset: managed-account OAuth is Claude-side only for now.
    expect(preset).not.toHaveProperty("providerType");
    expect(preset!.auth).toEqual({ OPENAI_API_KEY: "" });
    expect(extractCodexBaseUrl(preset!.config)).toBe("https://api.x.ai/v1");
    expect(extractCodexWireApi(preset!.config)).toBe("responses");
    expect(extractCodexModelName(preset!.config)).toBe("grok-4.5");
    expect(preset!.modelCatalog).toEqual([
      expect.objectContaining({
        model: "grok-4.5",
        contextWindow: 500000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
      }),
    ]);
  });

  it("pins the Codex OAuth preset to managed native Responses", () => {
    const preset = codexProviderPresets.find(
      (entry) => entry.name === "xAI (Grok) OAuth",
    );
    expect(preset).toBeDefined();
    expect(preset).toMatchObject({
      category: "third_party",
      apiFormat: "openai_responses",
      providerType: "xai_oauth",
      requiresOAuth: true,
      icon: "xai",
    });
    // Managed OAuth: auth.json keeps an empty key; the forwarder injects the
    // real access token per request and the adapter pins the base URL.
    expect(preset!.auth).toEqual({ OPENAI_API_KEY: "" });
    expect(extractCodexBaseUrl(preset!.config)).toBe("https://api.x.ai/v1");
    expect(extractCodexWireApi(preset!.config)).toBe("responses");
    expect(extractCodexModelName(preset!.config)).toBe("grok-4.5");
    expect(preset!.modelCatalog).toEqual([
      expect.objectContaining({
        model: "grok-4.5",
        contextWindow: 500000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
      }),
    ]);
  });
});
