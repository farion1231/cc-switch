import { describe, expect, it } from "vitest";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";

// Hermes routes `model.provider` to a built-in provider when the name is a
// canonical built-in id, ignoring a same-named custom_provider.
const HERMES_BUILTIN_MINIMAX_IDS = ["minimax", "minimax-cn", "minimax-oauth"];

describe("Hermes MiniMax presets", () => {
  it.each(["MiniMax", "MiniMax en"])(
    "%s prefills a provider key that no Hermes built-in shadows",
    (name) => {
      const preset = hermesProviderPresets.find((p) => p.name === name);
      expect(preset).toBeDefined();

      const key = preset!.providerKey;
      expect(key).toMatch(/^[a-z0-9]+(-[a-z0-9]+)*$/);
      expect(HERMES_BUILTIN_MINIMAX_IDS).not.toContain(key);
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
