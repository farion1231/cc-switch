import { describe, expect, it } from "vitest";
import { codexProviderPresets } from "./codexProviderPresets";
import {
  isTraeOfficialConfig,
  normalizeTraeProviderKey,
  traeProviderPresets,
  TRAE_PROVIDER_KEY_PATTERN,
  TRAE_RESERVED_PROVIDER_KEYS,
} from "./traeProviderPresets";
import { TRAE_OFFICIAL_PROVIDER_ID } from "@/utils/providerCapabilities";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";

describe("traeProviderPresets", () => {
  it("starts with the TRAE account login", () => {
    const [official] = traeProviderPresets;
    expect(official.providerKey).toBe(TRAE_OFFICIAL_PROVIDER_ID);
    expect(official.category).toBe("official");
    expect(isTraeOfficialConfig(official.settingsConfig)).toBe(true);
  });

  it("only carries over native Responses endpoints with a model", () => {
    const custom = traeProviderPresets.slice(1);
    expect(custom.length).toBeGreaterThan(0);
    for (const preset of custom) {
      expect(isTraeOfficialConfig(preset.settingsConfig)).toBe(false);
      if (isTraeOfficialConfig(preset.settingsConfig)) continue;
      expect(preset.settingsConfig.provider.wire_api).toBe("responses");
      expect(preset.settingsConfig.provider.base_url).toMatch(/^https?:\/\//);
      expect(preset.settingsConfig.model.trim()).not.toBe("");
      expect(preset.providerKey).toMatch(TRAE_PROVIDER_KEY_PATTERN);
      expect(TRAE_RESERVED_PROVIDER_KEYS.has(preset.providerKey)).toBe(false);
      const source = codexProviderPresets.find(
        (p) =>
          p.name === preset.name &&
          extractCodexBaseUrl(p.config) ===
            preset.settingsConfig.provider.base_url,
      );
      expect(source?.apiFormat).toBe("openai_responses");
    }
  });

  it("normalizes suggested keys to the traecli key alphabet", () => {
    expect(normalizeTraeProviderKey(" Kimi (CN) ")).toBe("kimi-cn");
    expect(normalizeTraeProviderKey("my_provider")).toBe("my_provider");
  });
});
