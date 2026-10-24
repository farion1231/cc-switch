import { describe, expect, it } from "vitest";
import { ompProviderPresets } from "@/config/ompProviderPresets";
import { piProviderPresets } from "@/config/piProviderPresets";

describe("Oh My Pi provider preset translation", () => {
  it("translates Pi's DeepSeek switch and reasoning replay to native controls", () => {
    let translated = 0;
    for (const source of piProviderPresets) {
      const target = ompProviderPresets.find(
        (preset) => preset.providerKey === source.providerKey,
      )!;
      type Compat = Record<string, unknown> | undefined;
      const entries: [Compat, Compat][] = [
        [source.settingsConfig.compat, target.settingsConfig.compat],
        ...source.settingsConfig.models.map(
          (model, index): [Compat, Compat] => [
            model.compat,
            target.settingsConfig.models[index].compat as Compat,
          ],
        ),
      ];
      for (const [piCompat, ompCompat] of entries) {
        if (piCompat?.thinkingFormat === "deepseek") {
          translated += 1;
          expect(ompCompat?.thinkingFormat).toBe("zai");
        }
        if (piCompat?.requiresReasoningContentOnAssistantMessages === true) {
          expect(ompCompat).toMatchObject({
            reasoningContentField: "reasoning_content",
            requiresReasoningContentForToolCalls: true,
            requiresReasoningContentForAllAssistantTurns: true,
            allowsSyntheticReasoningContentForToolCalls: false,
          });
          expect(ompCompat).not.toHaveProperty(
            "requiresReasoningContentOnAssistantMessages",
          );
        }
      }
    }
    expect(translated).toBeGreaterThan(0);
  });

  it("drops the fields omp's models.yml schema does not define", () => {
    for (const preset of ompProviderPresets) {
      expect(preset.settingsConfig).not.toHaveProperty("name");
      for (const model of preset.settingsConfig.models) {
        expect(model).not.toHaveProperty("thinkingLevelMap");
      }
    }
  });
});
