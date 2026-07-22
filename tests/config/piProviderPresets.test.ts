import { describe, expect, it } from "vitest";
import { piProviderPresets } from "@/config/piProviderPresets";

describe("piProviderPresets", () => {
  it("leaves apiKey empty so users paste their own key", () => {
    for (const preset of piProviderPresets) {
      expect(preset.settingsConfig.apiKey).toBe("");
    }
  });

  it("does not use missing nameKey translations", () => {
    const knownNameKeys = new Set([
      "providerForm.presets.deepseek",
      "providerForm.presets.openrouter",
    ]);

    for (const preset of piProviderPresets) {
      if (preset.nameKey) {
        expect(knownNameKeys.has(preset.nameKey)).toBe(true);
      }
      expect(preset.name).not.toMatch(/^providerForm\.presets\./);
    }
  });

  it("provides a kebab-case providerKey for every preset", () => {
    for (const preset of piProviderPresets) {
      expect(preset.providerKey).toMatch(/^[a-z0-9]+(-[a-z0-9]+)*$/);
    }
  });

  it("aligns Chinese provider display names with other apps", () => {
    const names = piProviderPresets.map((p) => p.name);
    expect(names).toContain("Zhipu GLM");
    expect(names).toContain("Zhipu GLM en");
    expect(names).toContain("火山Agentplan");
    expect(names).not.toContain("智谱 GLM");
    expect(names).not.toContain("火山方舟 (Volcengine)");
  });

  it("uses API Key auth for every preset (no OAuth official category)", () => {
    for (const preset of piProviderPresets) {
      expect(preset.category).not.toBe("official");
      expect(preset.isOfficial).toBeFalsy();
      expect(preset.apiKeyUrl).toBeTruthy();
      expect(preset.settingsConfig.apiKey).toBe("");
    }
  });

  it("keeps DeepSeek as cn_official with a key portal link", () => {
    const deepseek = piProviderPresets.find((p) => p.providerKey === "deepseek");
    expect(deepseek).toBeDefined();
    expect(deepseek!.category).toBe("cn_official");
    expect(deepseek!.apiKeyUrl).toContain("platform.deepseek.com");
  });
});
