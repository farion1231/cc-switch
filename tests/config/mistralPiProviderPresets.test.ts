import { describe, expect, it } from "vitest";
import { piProviderPresets } from "@/config/piProviderPresets";

describe("Mistral AI Pi preset", () => {
  const preset = piProviderPresets.find((p) => p.name === "Mistral AI")!;

  it("uses Pi's native Mistral protocol without shadowing the built-in provider", () => {
    expect(preset).toBeDefined();
    expect(preset.providerKey).toBe("cc-switch-mistral");
    expect(preset.apiKeyUrl).toBe("https://chat.mistral.ai/code/extensions");
    expect(preset.settingsConfig).toMatchObject({
      baseUrl: "https://api.mistral.ai",
      api: "mistral-conversations",
      apiKey: "",
    });
    expect(preset.settingsConfig.compat).toBeUndefined();
  });

  it("matches Pi's built-in model limits and reasoning_effort levels", () => {
    const models = JSON.parse(JSON.stringify(preset.settingsConfig.models));
    expect(models).toEqual([
      {
        id: "mistral-large-4",
        name: "Mistral Large 4",
        reasoning: true,
        input: ["text", "image"],
        contextWindow: 1048576,
        maxTokens: 262144,
        thinkingLevelMap: {
          off: null,
          minimal: null,
          low: "low",
          medium: "medium",
          high: "high",
          xhigh: null,
          max: null,
        },
      },
      {
        id: "mistral-small-2603",
        name: "Mistral Small 4",
        reasoning: true,
        input: ["text", "image"],
        contextWindow: 262144,
        maxTokens: 256000,
        thinkingLevelMap: {
          off: "none",
          minimal: null,
          low: null,
          medium: null,
          high: "high",
          xhigh: null,
          max: null,
        },
      },
    ]);
  });
});
