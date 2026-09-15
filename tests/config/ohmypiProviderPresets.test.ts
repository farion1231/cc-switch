import { describe, expect, it } from "vitest";
import { ohmypiProviderPresets } from "@/config/ohmypiProviderPresets";

// Oh My Pi's `models.yml` is validated by `ModelsConfigSchema`. These model
// fields are not part of that schema and must never be written into OMP's
// config, no matter where a preset was sourced from. Guarding them keeps a
// field with no OMP meaning out of `models.yml`.
const UNSUPPORTED_MODEL_KEYS = ["thinkingLevelMap"] as const;
const UNSUPPORTED_COMPAT_KEYS = [
  "forceAdaptiveThinking",
  "requiresReasoningContentOnAssistantMessages",
  "deferredToolsMode",
] as const;

describe("ohmypiProviderPresets", () => {
  it("exposes a non-empty catalog of well-formed presets", () => {
    expect(ohmypiProviderPresets.length).toBeGreaterThan(0);
    for (const preset of ohmypiProviderPresets) {
      expect(preset.settingsConfig.baseUrl.length).toBeGreaterThan(0);
      expect(preset.settingsConfig.api.length).toBeGreaterThan(0);
      expect(preset.settingsConfig.models.length).toBeGreaterThan(0);
    }
  });

  it("never writes model fields unsupported by OMP's models.yml schema", () => {
    for (const preset of ohmypiProviderPresets) {
      for (const model of preset.settingsConfig.models) {
        for (const key of UNSUPPORTED_MODEL_KEYS) {
          expect(key in model).toBe(false);
        }
      }
    }
  });

  it("never writes compat keys unsupported by OMP's models.yml schema", () => {
    const leaked: string[] = [];
    for (const preset of ohmypiProviderPresets) {
      for (const model of preset.settingsConfig.models) {
        for (const key of UNSUPPORTED_COMPAT_KEYS) {
          if (model.compat && key in model.compat) {
            leaked.push(`${preset.name}:${model.id}:${key}`);
          }
        }
      }
    }
    expect(leaked).toEqual([]);
  });

  it("keeps the compat keys that OMP does support", () => {
    // Existing presets carry a compat block; the shared keys (e.g.
    // supportsStore / maxTokensField / thinkingFormat) survive so OMP models
    // keep their real wire behavior.
    const presetsWithCompat = ohmypiProviderPresets.filter((preset) =>
      preset.settingsConfig.models.some((model) => model.compat !== undefined),
    );
    expect(presetsWithCompat.length).toBeGreaterThan(0);
  });
});