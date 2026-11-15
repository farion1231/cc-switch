import { describe, expect, it } from "vitest";
import { isHermesBuiltinProviderId } from "@/config/hermesBuiltinProviderIds";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";

describe("Hermes MiniMax presets", () => {
  it.each(["MiniMax", "MiniMax en"])(
    "%s prefills a provider key that no Hermes built-in shadows",
    (name) => {
      const preset = hermesProviderPresets.find((p) => p.name === name);
      expect(preset).toBeDefined();

      const key = preset!.providerKey;
      expect(key).toMatch(/^[a-z0-9]+(-[a-z0-9]+)*$/);
      expect(isHermesBuiltinProviderId(key!)).toBe(false);
      expect(preset!.settingsConfig.name).toBe(key);
      expect(preset!.suggestedDefaults?.model.provider).toBe(key);
    },
  );

  it.each(["MiniMax", "MiniMax en"])(
    "%s declares image input for MiniMax-M3.1-Flash",
    (name) => {
      const preset = hermesProviderPresets.find((p) => p.name === name);
      const flash = preset!.settingsConfig.models?.find(
        (m) => m.id === "MiniMax-M3.1-Flash",
      );
      expect(flash?.supports_vision).toBe(true);
    },
  );
});
