import { describe, expect, it } from "vitest";
import {
  chatCompletionsUrl,
  workbuddyProviderPresets,
} from "./workbuddyProviderPresets";
import { piProviderPresets } from "./piProviderPresets";

describe("workbuddyProviderPresets", () => {
  it("derives only OpenAI-compatible Pi presets", () => {
    expect(workbuddyProviderPresets.length).toBeGreaterThan(0);
    for (const preset of workbuddyProviderPresets) {
      const pi = piProviderPresets.find((p) => p.name === preset.name)!;
      expect(["openai-completions", "openai-responses"]).toContain(
        pi.settingsConfig.api,
      );
      expect(preset.settingsConfig.apiKey).toBe("");
      expect(preset.settingsConfig.models.map((model) => model.id)).toEqual(
        pi.settingsConfig.models.map((model) => model.id),
      );
    }
  });

  it("writes the full Chat Completions URL unless the Responses API is used", () => {
    for (const preset of workbuddyProviderPresets) {
      const { url, api } = preset.settingsConfig;
      if (api === "openai-responses") {
        expect(url).not.toMatch(/\/responses$/);
      } else {
        expect(url).toMatch(/^https?:\/\/.+\/chat\/completions$/);
      }
    }
  });

  it("normalizes base URLs", () => {
    expect(chatCompletionsUrl("https://a.test/v1/")).toBe(
      "https://a.test/v1/chat/completions",
    );
    expect(chatCompletionsUrl("https://a.test/v1/chat/completions")).toBe(
      "https://a.test/v1/chat/completions",
    );
  });
});
