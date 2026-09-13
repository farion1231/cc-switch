import { describe, expect, it } from "vitest";

import {
  getDeepSeekHarnessPresetEntries,
  deepseekHarnessProviderPresets,
} from "@/config/deepseekHarnessProviderPresets";

const DSH_MODEL_APIS = [
  "openai-completions",
  "openai-responses",
  "anthropic-messages",
] as const;

describe("DeepSeek Harness provider presets", () => {
  it("keeps the official DeepSeek preset first", () => {
    const preset = deepseekHarnessProviderPresets[0];
    expect(preset).toMatchObject({
      id: "deepseek-official",
      name: "DeepSeek",
      category: "official",
      isOfficial: true,
      icon: "deepseek",
      websiteUrl: "https://platform.deepseek.com",
      apiKeyUrl: "https://platform.deepseek.com/api_keys",
      settingsConfig: {
        baseURL: "https://api.deepseek.com",
        profile: "desktop",
        models: [
          { id: "deepseek-v4-flash", name: "DeepSeek-V4-Flash" },
          { id: "deepseek-v4-pro", name: "DeepSeek-V4-Pro" },
        ],
      },
    });
  });

  it("exposes the generic OpenAI-compatible preset", () => {
    const preset = deepseekHarnessProviderPresets[1];
    expect(preset).toMatchObject({
      id: "dsh-openai-compatible",
      name: "OpenAI Compatible",
      category: "custom",
      settingsConfig: {
        displayName: "OpenAI Compatible",
        api: "openai-completions",
        baseURL: "https://api.openai.com/v1",
        apiKeyEnv: "OPENAI_API_KEY",
      },
    });
  });

  it("derives a large catalog from the OpenCode presets", () => {
    expect(deepseekHarnessProviderPresets.length).toBeGreaterThan(20);

    const names = deepseekHarnessProviderPresets.map((preset) => preset.name);
    for (const expected of [
      "Kimi",
      "Zhipu GLM",
      "OpenRouter",
      "MiniMax",
      // Upstream v3.20.3 rebranded Bailian to 千问AI平台; the DSH catalog
      // derives the new name automatically.
      "千问AI平台",
    ]) {
      expect(names).toContain(expected);
    }
  });

  it("maps every non-official preset to a valid custom pi-ai shape", () => {
    const custom = deepseekHarnessProviderPresets.filter(
      (preset) => !preset.isOfficial,
    );
    expect(custom.length).toBeGreaterThan(20);

    for (const preset of custom) {
      expect(typeof preset.settingsConfig.displayName).toBe("string");
      expect(preset.settingsConfig.displayName).not.toBe("");

      expect(DSH_MODEL_APIS).toContain(preset.settingsConfig.api);
      expect(preset.settingsConfig.baseURL?.trim()).not.toBe("");
      expect(preset.settingsConfig.apiKeyEnv).toMatch(/^[A-Z_][A-Z0-9_]*$/);
      expect(Array.isArray(preset.settingsConfig.models)).toBe(true);
    }
  });

  it("preserves model order and falls back to the model id for names", () => {
    const kimi = deepseekHarnessProviderPresets.find(
      (preset) => preset.name === "Kimi",
    );
    expect(kimi).toBeDefined();
    expect(kimi?.settingsConfig).toMatchObject({
      displayName: "Kimi",
      api: "openai-completions",
      baseURL: "https://api.moonshot.cn/v1",
      apiKeyEnv: "KIMI_API_KEY",
      models: [
        { id: "kimi-k2.7-code", name: "Kimi K2.7 Code" },
        { id: "kimi-k3", name: "Kimi K3" },
      ],
    });
  });

  it("assigns a unique credential reference to every preset", () => {
    const refs = deepseekHarnessProviderPresets
      .map((preset) => preset.settingsConfig.apiKeyEnv)
      .filter((ref): ref is string => typeof ref === "string" && ref !== "");
    expect(new Set(refs).size).toBe(refs.length);
  });

  it("excludes OMO templates that DSH cannot represent", () => {
    expect(
      deepseekHarnessProviderPresets.some(
        (preset) => preset.category === "omo" || preset.category === "omo-slim",
      ),
    ).toBe(false);
  });

  it("maps presets for the provider form", () => {
    const entries = getDeepSeekHarnessPresetEntries();
    expect(entries).toHaveLength(deepseekHarnessProviderPresets.length);
    entries.forEach((entry, index) => {
      expect(entry.id).toBe(`deepseek-harness-${index}`);
      expect(entry.preset).toBe(deepseekHarnessProviderPresets[index]);
    });
  });
});
