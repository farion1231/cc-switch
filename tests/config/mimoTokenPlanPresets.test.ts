import { describe, expect, it } from "vitest";
import {
  openclawProviderPresets,
  rebaseOpenClawSuggestedDefaults,
} from "@/config/openclawProviderPresets";

describe("Xiaomi MiMo Token Plan presets", () => {
  it("uses a separate OpenClaw provider namespace from pay-as-you-go MiMo", () => {
    const payAsYouGo = openclawProviderPresets.find(
      (item) => item.name === "Xiaomi MiMo",
    );
    const tokenPlan = openclawProviderPresets.find(
      (item) => item.name === "Xiaomi MiMo Token Plan",
    );

    expect(payAsYouGo?.suggestedDefaults?.model?.primary).toBe(
      "xiaomimimo/mimo-v2-pro",
    );
    expect(tokenPlan?.suggestedDefaults?.model?.primary).toBe(
      "xiaomi-mimo-token-plan/mimo-v2.5-pro",
    );
    expect(tokenPlan?.suggestedDefaults?.modelCatalog).toHaveProperty(
      "xiaomi-mimo-token-plan/mimo-v2.5-pro",
    );
    expect(tokenPlan?.suggestedDefaults?.modelCatalog).toHaveProperty(
      "xiaomi-mimo-token-plan/mimo-v2.5",
    );
  });

  it("rebases OpenClaw defaults to the submitted provider key", () => {
    const tokenPlan = openclawProviderPresets.find(
      (item) => item.name === "Xiaomi MiMo Token Plan",
    );

    expect(tokenPlan?.suggestedDefaults).toBeDefined();

    const rebased = rebaseOpenClawSuggestedDefaults(
      tokenPlan!.suggestedDefaults!,
      "my-mimo-plan",
    );

    expect(rebased.model?.primary).toBe("my-mimo-plan/mimo-v2.5-pro");
    expect(rebased.modelCatalog).toHaveProperty(
      "my-mimo-plan/mimo-v2.5-pro",
    );
    expect(rebased.modelCatalog).toHaveProperty("my-mimo-plan/mimo-v2.5");
    expect(rebased.modelCatalog).not.toHaveProperty(
      "xiaomi-mimo-token-plan/mimo-v2.5-pro",
    );
  });
});
