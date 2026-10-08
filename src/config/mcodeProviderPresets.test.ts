import { describe, expect, it } from "vitest";
import { mcodeProviderPresets } from "./mcodeProviderPresets";
import { piProviderPresets } from "./piProviderPresets";

const mcodePreset = (name: string) =>
  mcodeProviderPresets.find((preset) => preset.name === name);
const piPreset = (name: string) =>
  piProviderPresets.find((preset) => preset.name === name)!;

describe("mcodeProviderPresets", () => {
  it("passes Pi's model compat through verbatim but never its thinkingLevelMap", () => {
    for (const preset of mcodeProviderPresets) {
      const piModels = piPreset(preset.name).settingsConfig.models;
      for (const [id, model] of Object.entries(preset.settingsConfig.models)) {
        const piModel = piModels.find((candidate) => candidate.id === id)!;
        expect(model.compat, `${preset.name}/${id}`).toEqual(piModel.compat);
        expect(model, `${preset.name}/${id}`).not.toHaveProperty(
          "thinkingLevelMap",
        );
      }
    }
  });

  it("gives adaptive-thinking Claude models effort options alongside their compat", () => {
    const pi = piPreset("PackyCode");
    const mcode = mcodePreset("PackyCode");
    expect(mcode).toBeDefined();
    expect(Object.keys(mcode!.settingsConfig.models)).toEqual(
      pi.settingsConfig.models.map((model) => model.id),
    );
    const opus = mcode!.settingsConfig.models["claude-opus-5-5"];
    expect(opus.compat).toEqual({ forceAdaptiveThinking: true });
    expect(opus.thinking).toEqual({
      effortOptions: ["low", "medium", "high", "xhigh", "max"],
    });
  });

  it("keeps dialect presets, relying on compat instead of effort options", () => {
    const deepseek = Object.values(mcodePreset("PPIO")!.settingsConfig.models);
    expect(deepseek[0].compat).toMatchObject({
      thinkingFormat: "deepseek",
      requiresReasoningContentOnAssistantMessages: true,
    });
    expect(deepseek[0]).not.toHaveProperty("thinking");

    const qwen =
      mcodePreset("千问AI平台")!.settingsConfig.models["qwen3.8-max"];
    expect(qwen.compat).toEqual({
      thinkingFormat: "qwen",
      supportsDeveloperRole: false,
    });
  });

  it("skips only the models whose compat MCode cannot express", () => {
    const models = Object.keys(mcodePreset("Kimi")!.settingsConfig.models);
    expect(models).toContain("kimi-k2.7-code");
    expect(models).not.toContain("kimi-k3");
    expect(
      piPreset("Kimi").settingsConfig.models.map((model) => model.id),
    ).toContain("kimi-k3");
  });

  it("excludes presets on APIs MCode does not support", () => {
    expect(mcodePreset("AWS Bedrock")).toBeUndefined();
  });
});
