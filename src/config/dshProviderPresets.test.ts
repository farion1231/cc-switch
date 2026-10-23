import { describe, expect, it } from "vitest";
import { dshProviderPresets } from "./dshProviderPresets";
import { piProviderPresets } from "./piProviderPresets";

describe("DeepSeek Harness presets", () => {
  it("preserves provider metadata, model capacities, input types and compatibility", () => {
    expect(dshProviderPresets.length).toBeGreaterThan(0);
    for (const preset of dshProviderPresets) {
      const source = piProviderPresets.find(
        (source) => source.providerKey === preset.providerKey,
      )!;
      expect(preset.settingsConfig.api).toMatch(
        /^(openai-completions|openai-responses|anthropic-messages)$/,
      );
      expect(preset.settingsConfig.options.baseURL).toBe(
        source.settingsConfig.baseUrl,
      );
      expect(preset.settingsConfig.options.headers).toEqual(
        source.settingsConfig.headers,
      );
      for (const model of source.settingsConfig.models) {
        const converted = preset.settingsConfig.models[model.id];
        expect(converted.limit).toEqual({
          context: model.contextWindow,
          output: model.maxTokens,
        });
        expect(converted.modalities).toEqual({
          input: model.input,
          output: ["text"],
        });
        expect(converted.compat).toEqual(model.compat);
      }
    }
  });
  it("does not advertise transports the native adapter cannot use", () => {
    const unsupported = piProviderPresets.filter((preset) =>
      ["google-generative-ai", "bedrock-converse-stream"].includes(
        preset.settingsConfig.api,
      ),
    );
    for (const preset of unsupported) {
      expect(
        dshProviderPresets.some(
          (candidate) => candidate.providerKey === preset.providerKey,
        ),
      ).toBe(false);
    }
  });
});
