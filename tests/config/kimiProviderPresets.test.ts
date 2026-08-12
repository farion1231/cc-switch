import { describe, expect, it } from "vitest";
import {
  kimiProviderPresets,
  KIMI_DEFAULT_API_TYPE,
  kimiApiTypes,
} from "@/config/kimiProviderPresets";

describe("Kimi Provider Presets", () => {
  it("should include the official Kimi For Coding preset", () => {
    const official = kimiProviderPresets.find(
      (p) => p.settingsConfig.name === "kimi_coding",
    );
    expect(official).toBeDefined();
    expect(official!.settingsConfig.type).toBe("kimi");
    expect(official!.settingsConfig.base_url).toBe(
      "https://api.kimi.com/coding/v1",
    );
  });

  it("official preset should have a default_model matching a declared model", () => {
    for (const preset of kimiProviderPresets) {
      const { models, default_model } = preset.settingsConfig;
      if (!models || models.length === 0) continue;
      if (default_model) {
        const ids = models.map((m) => m.id);
        expect(ids).toContain(default_model);
      }
    }
  });

  it("every preset should have a valid protocol type", () => {
    const validTypes = kimiApiTypes.map((t) => t.value);
    for (const preset of kimiProviderPresets) {
      const ty = preset.settingsConfig.type ?? KIMI_DEFAULT_API_TYPE;
      expect(validTypes).toContain(ty);
    }
  });

  it("every preset should declare at least one model with an id", () => {
    for (const preset of kimiProviderPresets) {
      const models = preset.settingsConfig.models;
      expect(models).toBeDefined();
      expect(models!.length).toBeGreaterThan(0);
      for (const model of models!) {
        expect(model.id).toBeTruthy();
      }
    }
  });

  it("preset name should be unique", () => {
    const names = kimiProviderPresets.map((p) => p.name);
    expect(new Set(names).size).toBe(names.length);
  });

  it("Kimi model max_context_size should be positive when set", () => {
    for (const preset of kimiProviderPresets) {
      for (const model of preset.settingsConfig.models ?? []) {
        if (model.max_context_size !== undefined) {
          expect(model.max_context_size).toBeGreaterThan(0);
        }
      }
    }
  });
});
