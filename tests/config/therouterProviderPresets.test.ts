import { describe, expect, it } from "vitest";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";

describe("TheRouter provider presets", () => {
  it("uses the Anthropic-compatible root endpoint for Claude", () => {
    const preset = providerPresets.find((item) => item.name === "TheRouter");

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe("https://therouter.ai");
    expect(preset?.apiKeyUrl).toBe("https://dashboard.therouter.ai");
    expect(preset?.category).toBe("aggregator");
    expect(preset?.endpointCandidates).toEqual(["https://api.therouter.ai"]);

    const env = (preset?.settingsConfig as { env: Record<string, string> }).env;
    expect(env.ANTHROPIC_BASE_URL).toBe("https://api.therouter.ai");
    expect(env.ANTHROPIC_AUTH_TOKEN).toBe("");
    expect(env.ANTHROPIC_API_KEY).toBe("");
    expect(env.ANTHROPIC_MODEL).toBe("anthropic/claude-sonnet-4.6");
    expect(env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe(
      "anthropic/claude-haiku-4.5",
    );
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(
      "anthropic/claude-sonnet-4.6",
    );
    expect(env.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe(
      "anthropic/claude-opus-4.6",
    );
  });

  it("uses the OpenAI-compatible v1 endpoint for Codex", () => {
    const preset = codexProviderPresets.find((item) => item.name === "TheRouter");

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe("https://therouter.ai");
    expect(preset?.apiKeyUrl).toBe("https://dashboard.therouter.ai");
    expect(preset?.category).toBe("aggregator");
    expect(preset?.endpointCandidates).toEqual([
      "https://api.therouter.ai/v1",
    ]);
    expect(preset?.auth).toEqual({ OPENAI_API_KEY: "" });
    expect(preset?.config).toContain('model_provider = "therouter"');
    expect(preset?.config).toContain('model = "openai/gpt-5.3-codex"');
    expect(preset?.config).toContain(
      'base_url = "https://api.therouter.ai/v1"',
    );
    expect(preset?.config).toContain('wire_api = "responses"');
  });

  it("uses the Gemini-native root endpoint for Gemini", () => {
    const preset = geminiProviderPresets.find((item) => item.name === "TheRouter");

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe("https://therouter.ai");
    expect(preset?.apiKeyUrl).toBe("https://dashboard.therouter.ai");
    expect(preset?.category).toBe("aggregator");
    expect(preset?.endpointCandidates).toEqual(["https://api.therouter.ai"]);
    expect(preset?.baseURL).toBe("https://api.therouter.ai");
    expect(preset?.model).toBe("gemini-2.5-pro");

    const env = (preset?.settingsConfig as { env: Record<string, string> }).env;
    expect(env.GOOGLE_GEMINI_BASE_URL).toBe("https://api.therouter.ai");
    expect(env.GEMINI_MODEL).toBe("gemini-2.5-pro");
  });
});
