import { describe, expect, it } from "vitest";
import { piProviderPresets } from "@/config/piProviderPresets";
import { ompProviderPresets } from "@/config/ompProviderPresets";
import { mcodeProviderPresets } from "@/config/mcodeProviderPresets";
import { dshProviderPresets } from "@/config/dshProviderPresets";
import {
  kimiProviderPresets,
  kimiConfigSchema,
} from "@/config/kimiProviderPresets";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";

describe("additional TokenPlus apps", () => {
  for (const [app, presets] of Object.entries({
    pi: piProviderPresets,
    omp: ompProviderPresets,
    mcode: mcodeProviderPresets,
    dsh: dshProviderPresets,
    kimi: kimiProviderPresets,
    desktop: claudeDesktopProviderPresets,
  })) {
    it(`offers a branded preset for ${app}`, () => {
      const preset = presets.find((p) => p.name === "TokenPlus");
      expect(preset?.icon).toBe("tokenplus");
      expect(preset?.partnerPromotionKey).toBe("tokenplus");
    });
  }
  it("produces valid Kimi configuration with four models", () => {
    const config = kimiProviderPresets.find(
      (p) => p.name === "TokenPlus",
    )?.settingsConfig;
    expect(kimiConfigSchema.safeParse(config).success).toBe(true);
    expect(Object.keys(config?.models ?? {})).toHaveLength(4);
  });
});
