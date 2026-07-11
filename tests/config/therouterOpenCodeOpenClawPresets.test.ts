import { describe, expect, it } from "vitest";
import {
  OPENCODE_PRESET_MODEL_VARIANTS,
  opencodeProviderPresets,
} from "@/config/opencodeProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";

describe("TheRouter OpenCode and OpenClaw presets", () => {
  it("uses OpenAI-compatible config for OpenCode", () => {
    const preset = opencodeProviderPresets.find(
      (item) => item.name === "TheRouter",
    );
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
    expect(models).toHaveProperty("anthropic/claude-sonnet-5");
    expect(models).toHaveProperty("google/gemini-3.5-flash");
    expect(models["google/gemini-3.5-flash"]?.name).toBe("Gemini 3.5 Flash");
  });

  it("uses OpenAI completions config for OpenClaw", () => {
    const preset = openclawProviderPresets.find(
      (item) => item.name === "TheRouter",
    );
    const openClawModels = preset?.settingsConfig.models ?? [];
    const modelIds = openClawModels.map((model) => model.id);

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe("https://therouter.ai");
    expect(preset?.apiKeyUrl).toBe("https://dashboard.therouter.ai");
    expect(preset?.category).toBe("aggregator");
    expect(preset?.settingsConfig.baseUrl).toBe("https://api.therouter.ai/v1");
    expect(preset?.settingsConfig.api).toBe("openai-completions");
    expect(modelIds).toEqual(
      expect.arrayContaining([
        "anthropic/claude-sonnet-5",
        "openai/gpt-5.3-codex",
        "openai/gpt-5.2",
        "google/gemini-3.5-flash",
      ]),
    );
    expect(
      openClawModels.find((model) => model.id === "google/gemini-3.5-flash"),
    ).toMatchObject({
      name: "Gemini 3.5 Flash",
      cost: { input: 1.5, output: 9, cacheRead: 0.15 },
    });
    expect(preset?.suggestedDefaults?.model).toEqual({
      primary: "therouter/anthropic/claude-sonnet-5",
      fallbacks: [
        "therouter/openai/gpt-5.2",
        "therouter/google/gemini-3.5-flash",
      ],
    });
  });

  it("keeps Google OpenCode preset model ids unique", () => {
    const googleModels = OPENCODE_PRESET_MODEL_VARIANTS["@ai-sdk/google"];
    const ids = googleModels.map((model) => model.id);
    const geminiFlashModels = googleModels.filter(
      (model) => model.id === "gemini-3.5-flash",
    );

    expect(new Set(ids).size).toBe(ids.length);
    expect(geminiFlashModels).toHaveLength(1);
    expect(geminiFlashModels[0]).toMatchObject({
      name: "Gemini 3.5 Flash",
      variants: {
        minimal: expect.any(Object),
        low: expect.any(Object),
        medium: expect.any(Object),
        high: expect.any(Object),
      },
    });
  });
});
