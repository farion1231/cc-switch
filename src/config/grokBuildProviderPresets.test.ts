import { describe, expect, it } from "vitest";
import {
  grokBuildOfficialPreset,
  grokBuildProviderPresets,
} from "./grokBuildProviderPresets";
import {
  extractCodexBaseUrl,
  extractCodexModelName,
} from "../utils/providerConfigUtils";
import { GROK_BUILD_DEFAULT_MODEL } from "../utils/grokBuildConfig";

describe("grokBuildProviderPresets", () => {
  it("has unique preset names", () => {
    const names = grokBuildProviderPresets.map((p) => p.name);
    expect(new Set(names).size).toBe(names.length);
  });

  it("contains no official, managed-OAuth, or cn_official providers", () => {
    for (const preset of grokBuildProviderPresets) {
      expect(preset.category, preset.name).not.toBe("official");
      expect(preset.category, preset.name).not.toBe("cn_official");
      expect(preset.isOfficial, preset.name).toBeFalsy();
    }
  });

  it("excludes providers with no Grok models upstream", () => {
    const names = new Set(grokBuildProviderPresets.map((p) => p.name));
    const excluded = [
      "OpenAI Official",
      "Azure OpenAI",
      "xAI (Grok) OAuth",
      "DeepSeek",
      "Kimi",
      "Kimi For Coding",
      "Zhipu GLM",
      "MiniMax",
      "SiliconFlow",
      "SiliconFlow en",
      "ModelScope",
      "Novita AI",
      "Nvidia",
      "AtlasCloud",
      "OpenCode Go",
    ];
    for (const name of excluded) {
      expect(names.has(name), name).toBe(false);
    }
  });

  it("uses a Grok default model on every preset", () => {
    for (const preset of grokBuildProviderPresets) {
      const model = extractCodexModelName(preset.config);
      expect(
        model === GROK_BUILD_DEFAULT_MODEL || model === "x-ai/grok-4.5",
        `${preset.name}: ${model}`,
      ).toBe(true);
    }
  });

  it("carries a valid config carrier and empty API key slot", () => {
    for (const preset of grokBuildProviderPresets) {
      expect(extractCodexBaseUrl(preset.config), preset.name).toMatch(
        /^https:\/\//,
      );
      expect(preset.auth, preset.name).toEqual({ OPENAI_API_KEY: "" });
    }
  });

  it("keeps the official preset as an empty-config seed entry", () => {
    expect(grokBuildOfficialPreset.category).toBe("official");
    expect(grokBuildOfficialPreset.isOfficial).toBe(true);
    expect(grokBuildOfficialPreset.config).toBe("");
    expect(grokBuildOfficialPreset.auth).toEqual({});
  });
});
