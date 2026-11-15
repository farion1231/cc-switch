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
        for (const [key, value] of Object.entries(converted.compat ?? {})) {
          expect(model.compat).toHaveProperty(key, value);
        }
      }
    }
  });

  it("gives reasoning models with an empty Pi map explicit default levels", () => {
    let checked = 0;
    for (const preset of dshProviderPresets) {
      const source = piProviderPresets.find(
        (source) => source.providerKey === preset.providerKey,
      )!;
      for (const model of source.settingsConfig.models) {
        if (
          !model.reasoning ||
          Object.keys(model.thinkingLevelMap ?? {}).length > 0
        ) {
          continue;
        }
        expect(preset.settingsConfig.models[model.id].reasoningEfforts).toEqual(
          {
            off: null,
            minimal: "minimal",
            low: "low",
            medium: "medium",
            high: "high",
          },
        );
        checked++;
      }
    }
    expect(checked).toBeGreaterThan(0);
  });

  it("preserves supported reasoning levels without emitting empty or null-only maps", () => {
    for (const preset of dshProviderPresets) {
      const source = piProviderPresets.find(
        (source) => source.providerKey === preset.providerKey,
      )!;
      for (const model of source.settingsConfig.models) {
        const efforts = preset.settingsConfig.models[model.id].reasoningEfforts;
        if (!model.reasoning) {
          expect(efforts).toBe(false);
          continue;
        }
        expect(efforts).toBeTypeOf("object");
        expect(
          Object.entries(efforts as Record<string, string | null>).some(
            ([level, value]) => level !== "off" && typeof value === "string",
          ),
        ).toBe(true);
        for (const [level, value] of Object.entries(
          model.thinkingLevelMap ?? {},
        )) {
          if (value === null) {
            expect(efforts).not.toHaveProperty(level);
          } else {
            expect(efforts).toHaveProperty(level, value);
          }
        }
      }
    }
  });

  it("removes unsupported Kimi compat while retaining the supported wire options", () => {
    const kimiSources = piProviderPresets.filter((source) =>
      source.settingsConfig.models.some(
        (model) => model.compat?.deferredToolsMode === "kimi",
      ),
    );
    expect(kimiSources.length).toBeGreaterThanOrEqual(2);
    for (const source of kimiSources) {
      const preset = dshProviderPresets.find(
        (preset) => preset.providerKey === source.providerKey,
      )!;
      for (const model of source.settingsConfig.models) {
        if (model.compat?.deferredToolsMode !== "kimi") continue;
        const compat = preset.settingsConfig.models[model.id].compat;
        expect(compat).not.toHaveProperty("deferredToolsMode");
        expect(compat).toMatchObject({
          supportsStore: false,
          supportsDeveloperRole: false,
          supportsReasoningEffort: true,
          maxTokensField: "max_tokens",
          requiresReasoningContentOnAssistantMessages: true,
        });
      }
    }
  });

  it("only keeps protocol-specific compat on the protocols that offer it", () => {
    for (const preset of dshProviderPresets) {
      const api = preset.settingsConfig.api;
      const compatBlocks = [
        (preset.settingsConfig as { compat?: Record<string, unknown> }).compat,
        ...Object.values(preset.settingsConfig.models).map(
          (model) => model.compat,
        ),
      ];
      for (const compat of compatBlocks) {
        const fields = Object.keys(compat ?? {});
        expect(fields).not.toContain("deferredToolsMode");
        if (api !== "openai-completions") {
          expect(fields).not.toContain("supportsStore");
          expect(fields).not.toContain("maxTokensField");
          expect(fields).not.toContain("thinkingFormat");
        }
        if (api !== "anthropic-messages") {
          expect(fields).not.toContain("forceAdaptiveThinking");
        }
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
