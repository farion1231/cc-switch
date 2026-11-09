import { describe, expect, it } from "vitest";
import {
  ATOMCODE_PROVIDER_TYPES,
  atomCodeCustomTemplate,
  atomcodeProviderPresets,
} from "./atomcodeProviderPresets";

// 与后端 atomcode_config::validate_provider 同款约束
const KEY_PATTERN = /^[a-zA-Z0-9._-]+$/;
const PROVIDER_TYPES = new Set([
  "openai",
  "anthropic",
  "deepseek",
  "gemini",
  "openai_responses",
]);

describe("atomcodeProviderPresets", () => {
  it("uses account shapes that pass backend validate_provider", () => {
    for (const preset of atomcodeProviderPresets) {
      expect(KEY_PATTERN.test(preset.providerKey), preset.name).toBe(true);
      const { account, models } = preset.settingsConfig;
      expect(
        PROVIDER_TYPES.has(account.provider),
        `${preset.name}/account.provider`,
      ).toBe(true);
      expect(() => new URL(account.base_url), preset.name).not.toThrow();
      expect(Object.keys(models).length, preset.name).toBeGreaterThan(0);
      for (const [key, model] of Object.entries(models)) {
        expect(key.trim(), `${preset.name}/${key}`).not.toBe("");
        expect(model.model.trim(), `${preset.name}/${key}`).not.toBe("");
        expect(
          model.context_window ?? 1,
          `${preset.name}/${key}`,
        ).toBeGreaterThan(0);
        // account 字段由后端写入时统一改写，预设里不该带
        expect(model, `${preset.name}/${key}`).not.toHaveProperty("account");
      }
    }
  });

  it("keeps the DeepSeek official preset in sync with the 2026-10-25 Flash-only lineup", () => {
    const deepseek = atomcodeProviderPresets.find((p) => p.name === "DeepSeek");
    expect(deepseek).toBeDefined();
    expect(deepseek!.providerKey).toBe("deepseek");
    expect(deepseek!.settingsConfig.account.provider).toBe("deepseek");
    expect(deepseek!.settingsConfig.account.base_url).toBe(
      "https://api.deepseek.com",
    );
    expect(Object.keys(deepseek!.settingsConfig.models)).toEqual([
      "deepseek-flash",
    ]);
  });

  it("categorizes Anthropic and OpenAI under model vendors (cn_official)", () => {
    const anthropic = atomcodeProviderPresets.find(
      (p) => p.name === "Anthropic",
    );
    expect(anthropic).toBeDefined();
    expect(anthropic!.category).toBe("cn_official");
    expect(anthropic!.icon).toBe("anthropic");

    const openai = atomcodeProviderPresets.find((p) => p.name === "OpenAI");
    expect(openai).toBeDefined();
    expect(openai!.category).toBe("cn_official");
    expect(openai!.icon).toBe("openai");
  });

  it("declares exactly the provider types the backend accepts", () => {
    expect(ATOMCODE_PROVIDER_TYPES.map((type) => type.value)).toEqual([
      "openai",
      "openai_responses",
      "anthropic",
      "deepseek",
      "gemini",
    ]);
  });

  it("gives the custom template an empty account and no models", () => {
    const template = atomCodeCustomTemplate();
    expect(template.account.provider).toBe("openai");
    expect(template.account.base_url).toBe("");
    expect(template.models).toEqual({});
  });
});
