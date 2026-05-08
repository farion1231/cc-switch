import { describe, expect, it } from "vitest";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";

describe("TheRouter OpenCode and OpenClaw presets", () => {
  it("uses OpenAI-compatible config for OpenCode", () => {
    const preset = opencodeProviderPresets.find((item) => item.name === "TheRouter");
    const models = preset?.settingsConfig.models ?? {};

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe("https://therouter.ai");
    expect(preset?.apiKeyUrl).toBe("https://dashboard.therouter.ai");
    expect(preset?.category).toBe("aggregator");
    expect(preset?.settingsConfig.npm).toBe("@ai-sdk/openai-compatible");
    expect(preset?.settingsConfig.options?.baseURL).toBe(
      "https://api.therouter.ai/v1",
    );
    expect(preset?.settingsConfig.options?.setCacheKey).toBe(true);
    expect(models).toHaveProperty("openai/gpt-5.3-codex");
    expect(models).toHaveProperty("anthropic/claude-sonnet-4.6");
    expect(models).toHaveProperty("google/gemini-3-flash-preview");
  });

  it("uses OpenAI completions config for OpenClaw", () => {
    const preset = openclawProviderPresets.find((item) => item.name === "TheRouter");
    const modelIds = (preset?.settingsConfig.models ?? []).map((model) => model.id);

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe("https://therouter.ai");
    expect(preset?.apiKeyUrl).toBe("https://dashboard.therouter.ai");
    expect(preset?.category).toBe("aggregator");
    expect(preset?.settingsConfig.baseUrl).toBe("https://api.therouter.ai/v1");
    expect(preset?.settingsConfig.api).toBe("openai-completions");
    expect(modelIds).toEqual(
      expect.arrayContaining([
        "anthropic/claude-sonnet-4.6",
        "openai/gpt-5.3-codex",
        "openai/gpt-5.2",
        "google/gemini-3-flash-preview",
      ]),
    );
    expect(preset?.suggestedDefaults?.model).toEqual({
      primary: "therouter/anthropic/claude-sonnet-4.6",
      fallbacks: [
        "therouter/openai/gpt-5.2",
        "therouter/google/gemini-3-flash-preview",
      ],
    });
  });
});
