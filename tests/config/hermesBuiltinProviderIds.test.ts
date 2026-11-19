import { describe, expect, it } from "vitest";
import {
  HERMES_BUILTIN_PROVIDER_IDS,
  isHermesBuiltinProviderId,
} from "@/config/hermesBuiltinProviderIds";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";

describe("Hermes built-in provider ids", () => {
  it.each(["minimax", "minimax-cn", "minimax-oauth", "deepseek", "openrouter"])(
    "flags canonical built-in id %s",
    (key) => {
      expect(isHermesBuiltinProviderId(key)).toBe(true);
    },
  );

  it("normalizes like Hermes before matching", () => {
    expect(isHermesBuiltinProviderId(" DeepSeek ")).toBe(true);
  });

  // Aliases resolve to a different canonical id, so Hermes still picks the
  // same-named custom provider.
  it.each(["kimi", "glm", "zhipu", "moonshot", "cc-switch-minimax", "custom"])(
    "does not flag %s",
    (key) => {
      expect(isHermesBuiltinProviderId(key)).toBe(false);
    },
  );

  it("only lists ids a provider key can hold", () => {
    for (const id of HERMES_BUILTIN_PROVIDER_IDS) {
      expect(id).toMatch(/^[a-z0-9]+(-[a-z0-9]+)*$/);
    }
  });

  it("no preset prefills a provider key that Hermes shadows", () => {
    for (const preset of hermesProviderPresets) {
      if (!preset.providerKey) continue;
      expect({
        preset: preset.name,
        shadowed: isHermesBuiltinProviderId(preset.providerKey),
      }).toEqual({ preset: preset.name, shadowed: false });
    }
  });
});
