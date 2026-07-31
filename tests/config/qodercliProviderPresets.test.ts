import { describe, expect, it } from "vitest";
import {
  buildQoderCliModelProviderId,
  getQoderCliModelDisplayLabel,
  getQoderCliPlanLabel,
  isQoderCliSupportedModel,
  isQoderCliSupportedProvider,
  qodercliProviderPresets,
} from "@/config/qodercliProviderPresets";
import {
  buildQoderCliConfigJson,
  parseQoderCliConfig,
} from "@/components/providers/forms/helpers/qodercliFormUtils";

describe("Qoder CLI official BYOK catalog", () => {
  it("shows full plan names and leaves pay-as-you-go unlabeled", () => {
    expect(getQoderCliPlanLabel("cp")).toBe("Coding Plan");
    expect(getQoderCliPlanLabel("tp")).toBe("Token Plan");
    expect(getQoderCliPlanLabel("pg")).toBe("");
    expect(
      getQoderCliModelDisplayLabel({
        displayName: "Qwen 3.7 Plus",
        type: "cp",
      }),
    ).toBe("Qwen 3.7 Plus · Coding Plan");
    expect(
      getQoderCliModelDisplayLabel({
        displayName: "Qwen 3.7 Plus",
        type: "tp",
      }),
    ).toBe("Qwen 3.7 Plus · Token Plan");
    expect(
      getQoderCliModelDisplayLabel({
        displayName: "DeepSeek V4 Pro",
        type: "pg",
      }),
    ).toBe("DeepSeek V4 Pro");
  });

  it("contains only Qoder-supported provider keys", () => {
    expect(qodercliProviderPresets.map((preset) => preset.providerKey)).toEqual(
      [
        "deepseek",
        "bailian",
        "bailian-intl",
        "bailian-america",
        "zhipu",
        "zhipu-intl",
        "kimi",
        "minimax",
        "minimax-intl",
        "xiaomi-china",
      ],
    );
    expect(isQoderCliSupportedProvider("openai")).toBe(false);
    expect(isQoderCliSupportedProvider("ollama-local")).toBe(false);
  });

  it("pins every selectable model to an official plan type and format", () => {
    for (const preset of qodercliProviderPresets) {
      expect(preset.settingsConfig.provider).toBe(preset.providerKey);
      expect(preset.settingsConfig.models).toHaveLength(1);
      expect(preset.models.length).toBeGreaterThan(0);
      for (const model of preset.models) {
        expect(["cp", "tp", "pg"]).toContain(model.type);
        expect(model.format).toBe("openai");
        expect(isQoderCliSupportedModel(preset.providerKey, model)).toBe(true);
      }
    }
  });

  it("builds a Qoder config without an arbitrary base URL", () => {
    const preset = qodercliProviderPresets[0];
    const config = JSON.parse(
      buildQoderCliConfigJson(
        preset.providerKey,
        "  sk-test  ",
        preset.settingsConfig.models,
      ),
    );
    expect(config).toEqual({
      provider: "deepseek",
      apiKey: "sk-test",
      models: [preset.settingsConfig.models[0]],
    });
    expect(config).not.toHaveProperty("baseURL");
  });

  it("builds a stable record id per official model", () => {
    expect(
      buildQoderCliModelProviderId("deepseek", {
        model: "deepseek-v4-pro-pg",
      }),
    ).toBe("deepseek/deepseek-v4-pro-pg");
    expect(
      buildQoderCliModelProviderId("deepseek", {
        model: "deepseek-v4-flash-pg",
      }),
    ).toBe("deepseek/deepseek-v4-flash-pg");
  });

  it("does not accept an arbitrary legacy endpoint/model as official", () => {
    expect(
      parseQoderCliConfig({
        baseURL: "https://api.example.com/v1",
        apiKey: "sk-test",
        models: [{ model: "gpt-4o" }],
      }),
    ).toEqual({
      provider: "",
      apiKey: "sk-test",
      models: [],
    });
  });
});
