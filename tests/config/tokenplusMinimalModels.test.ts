import { describe, expect, it } from "vitest";
import { piProviderPresets } from "@/config/piProviderPresets";
import {
  piModelCatalog,
  getPiModelCatalogReference,
} from "@/config/piModelCatalog";
import { dshProviderPresets } from "@/config/dshProviderPresets";
describe("TokenPlus catalog models", () => {
  it("reuses complete catalog capabilities and propagates DSH limits", () => {
    const preset = piProviderPresets.find((p) => p.name === "TokenPlus")!;
    const dsh = dshProviderPresets.find((p) => p.name === "TokenPlus")!;
    for (const model of preset.settingsConfig.models) {
      const ref = getPiModelCatalogReference(model)!;
      const caps = piModelCatalog[ref.catalogKey].capabilities;
      expect(model.contextWindow).toBe(caps.contextWindow);
      expect(model.maxTokens).toBe(caps.maxTokens);
      expect(dsh.settingsConfig.models[model.id].limit).toEqual({
        context: model.contextWindow,
        output: model.maxTokens,
      });
    }
    expect(preset.settingsConfig.models[0]).toMatchObject({
      maxTokens: 131072,
      contextWindow: 1048576,
    });
  });
});
