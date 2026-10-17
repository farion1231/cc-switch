import { describe, expect, it } from "vitest";
import { piProviderPresets } from "./piProviderPresets";
import { ompProviderPresets } from "./ompProviderPresets";

describe("ompProviderPresets", () => {
  it("keeps provider and model parity with Pi with no exclusions", () => {
    expect(ompProviderPresets.map((p) => p.providerKey)).toEqual(
      piProviderPresets.map((p) => p.providerKey),
    );
    ompProviderPresets.forEach((preset, index) => {
      expect(preset.providerKey).toMatch(/^cc-switch-/);
      expect(preset.settingsConfig).not.toHaveProperty("name");
      expect(preset.settingsConfig.models.map((m) => m.id)).toEqual(
        piProviderPresets[index].settingsConfig.models.map((m) => m.id),
      );
      expect(preset.settingsConfig.api).toBe(
        piProviderPresets[index].settingsConfig.api,
      );
      expect(preset.settingsConfig.baseUrl).toBe(
        piProviderPresets[index].settingsConfig.baseUrl,
      );
    });
  });

  it("drops Pi-only thinking controls and incompatible wire flags", () => {
    for (const preset of ompProviderPresets) {
      for (const model of preset.settingsConfig.models) {
        expect(model).not.toHaveProperty("thinkingLevelMap");
        expect(model.compat ?? {}).not.toHaveProperty("forceAdaptiveThinking");
        expect(model.compat ?? {}).not.toHaveProperty(
          "requiresReasoningContentOnAssistantMessages",
        );
        expect(model.compat?.thinkingFormat).not.toBe("deepseek");
      }
    }
    expect(
      ompProviderPresets.some((p) =>
        p.settingsConfig.models.some(
          (m) => m.compat?.supportsDeveloperRole === false,
        ),
      ),
    ).toBe(true);
  });
});
