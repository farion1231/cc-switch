import { describe, expect, it } from "vitest";
import {
  chatCompletionsUrl,
  workbuddyProviderPresets,
} from "./workbuddyProviderPresets";
import { piProviderPresets } from "./piProviderPresets";

describe("workbuddyProviderPresets", () => {
  it("derives only Chat Completions Pi presets", () => {
    expect(workbuddyProviderPresets.length).toBeGreaterThan(0);
    for (const preset of workbuddyProviderPresets) {
      const pi = piProviderPresets.find((p) => p.name === preset.name)!;
      expect(pi.settingsConfig.api).toBe("openai-completions");
      expect(preset.settingsConfig).not.toHaveProperty("api");
      expect(preset.settingsConfig.apiKey).toBe("");
      expect(preset.settingsConfig.models.map((model) => model.id)).toEqual(
        pi.settingsConfig.models.map((model) => model.id),
      );
    }
  });

  it("writes the full Chat Completions URL", () => {
    for (const preset of workbuddyProviderPresets) {
      expect(preset.settingsConfig.url).toMatch(
        /^https?:\/\/.+\/chat\/completions$/,
      );
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
