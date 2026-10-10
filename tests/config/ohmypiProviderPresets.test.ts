import { describe, expect, it } from "vitest";
import { ohmypiProviderPresets } from "@/config/ohmypiProviderPresets";
import { piProviderPresets } from "@/config/piProviderPresets";

describe("Oh My Pi provider preset translation", () => {
  it("translates Pi's DeepSeek switch and reasoning replay to native controls", () => {
    let translated = 0;
    for (const source of piProviderPresets) {
      const target = ohmypiProviderPresets.find(
        (preset) => preset.providerKey === source.providerKey,
      )!;
      const entries = [
        [source.settingsConfig.compat, target.settingsConfig.compat],
        ...source.settingsConfig.models.map((model, index) => [
          model.compat,
          target.settingsConfig.models[index].compat,
        ]),
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
});
