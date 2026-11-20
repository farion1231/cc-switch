import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  OMP_API_FORMATS,
  ompProviderPresets,
} from "@/config/ompProviderPresets";
import { piProviderPresets } from "@/config/piProviderPresets";

describe("Oh My Pi provider preset translation", () => {
  it("translates Pi's DeepSeek switch and reasoning replay to native controls", () => {
    let translated = 0;
    for (const source of piProviderPresets) {
      // Presets on a protocol omp does not accept are not carried over.
      if (!OMP_API_FORMATS.includes(source.settingsConfig.api)) continue;
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

  it("only offers protocols omp's models.yml schema accepts", () => {
    // omp rejects the whole models.yml when one provider names an unknown `api`.
    const rust = readFileSync("src-tauri/src/omp_config.rs", "utf8");
    const apis = rust.match(/const APIS: &\[&str\] = &\[([^\]]*)\];/)![1];
    const accepted = [...apis.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    expect(accepted).toContain("openai-completions");

    for (const format of OMP_API_FORMATS) expect(accepted).toContain(format);
    for (const preset of ompProviderPresets) {
      expect(accepted).toContain(preset.settingsConfig.api);
    }
    expect(
      ompProviderPresets.some(
        (preset) => preset.settingsConfig.api === "mistral-conversations",
      ),
    ).toBe(false);
    expect(
      ompProviderPresets.some(
        (preset) => preset.providerKey === "cc-switch-mistral",
      ),
    ).toBe(false);
  });
});
