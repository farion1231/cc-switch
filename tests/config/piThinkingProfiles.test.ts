import { describe, expect, it } from "vitest";

import {
  getPiModelCatalogReference,
  piModelCatalog,
} from "@/config/piModelCatalog";
import { piProviderPresets } from "@/config/piProviderPresets";
import {
  isPiThinkingLevelMap,
  PI_THINKING_LEVELS,
  type PiThinkingLevelMap,
  piThinkingBindings,
  piThinkingProfiles,
  resolvePiThinkingProfile,
} from "@/config/piThinkingProfiles";

describe("Pi thinking profiles", () => {
  it("keeps valid, non-empty native maps", () => {
    for (const profile of Object.values(piThinkingProfiles)) {
      expect(isPiThinkingLevelMap(profile.map)).toBe(true);
      expect(Object.keys(profile.map).length).toBeGreaterThan(0);
      expect(
        Object.keys(profile.map).every((key) =>
          PI_THINKING_LEVELS.includes(
            key as (typeof PI_THINKING_LEVELS)[number],
          ),
        ),
      ).toBe(true);
    }
  });

  it("uses only explicit catalog and API bindings", () => {
    for (const binding of piThinkingBindings) {
      expect(resolvePiThinkingProfile(binding)).toEqual({
        profileId: binding.profileId,
        map: { ...piThinkingProfiles[binding.profileId].map },
        ...(binding.modelCompat
          ? { modelCompat: { ...binding.modelCompat } }
          : {}),
      });
      expect(
        resolvePiThinkingProfile({
          ...binding,
          api:
            binding.api === "openai-responses"
              ? "openai-completions"
              : "openai-responses",
        }),
      ).toBeUndefined();
    }
  });

  it("materializes preset-local profiles without serializing references", () => {
    const materialized = [];
    for (const preset of piProviderPresets) {
      for (const model of preset.settingsConfig.models) {
        if (!model.thinkingLevelMap) continue;
        const reference = getPiModelCatalogReference(model);
        materialized.push({
          preset: preset.name,
          modelId: model.id,
          profileId: reference?.presetThinkingProfileId,
          map: model.thinkingLevelMap,
        });
        expect(reference).toBeDefined();
        expect(piModelCatalog).toHaveProperty(reference!.catalogKey);
        expect(JSON.parse(JSON.stringify(model))).not.toHaveProperty(
          "presetThinkingProfileId",
        );
      }
    }

    expect(materialized).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          preset: "Kimi",
          modelId: "kimi-k2.7-code",
          profileId: "offUnsupported",
        }),
        expect.objectContaining({
          preset: "DeepSeek",
          modelId: "deepseek-flash",
          profileId: "deepseekV4",
        }),
        expect.objectContaining({
          preset: "OpenCode Go",
          modelId: "glm-5.3",
          profileId: "openCodeGoGlm52",
        }),
        expect.objectContaining({
          preset: "AWS Bedrock",
          modelId: "global.anthropic.claude-opus-5",
          profileId: "xhighAndMax",
        }),
      ]),
    );
  });

  it("gives every reasoning preset an explicit map", () => {
    for (const preset of piProviderPresets) {
      for (const model of preset.settingsConfig.models) {
        if (!model.reasoning) continue;
        expect(model).toHaveProperty("thinkingLevelMap");
        expect(isPiThinkingLevelMap(model.thinkingLevelMap)).toBe(true);
      }
    }
  });

  it("pairs Anthropic adaptive maps with Pi's required compatibility flag", () => {
    const adaptiveModels = piProviderPresets.flatMap((preset) =>
      preset.settingsConfig.api === "anthropic-messages"
        ? preset.settingsConfig.models.filter(
            (model) =>
              model.compat?.forceAdaptiveThinking === true &&
              Object.keys(model.thinkingLevelMap ?? {}).length > 0,
          )
        : [],
    );

    expect(adaptiveModels.length).toBeGreaterThan(0);
    for (const model of adaptiveModels) {
      expect(model.compat).toMatchObject({ forceAdaptiveThinking: true });
    }
  });

  it("does not add a generic binding for host-sensitive model families", () => {
    expect(
      piThinkingBindings.some((binding) =>
        /^(deepseek|zai|moonshotai)\//.test(binding.catalogKey),
      ),
    ).toBe(false);
  });

  it("materializes reviewed thinking maps for Qwen and MiMo completion presets", () => {
    const qwenPreset = piProviderPresets.find(
      (preset) => preset.name === "千问AI平台",
    );
    expect(qwenPreset).toBeDefined();
    const qwenModel = qwenPreset!.settingsConfig.models.find(
      (m) => m.id === "qwen3.8-max",
    );
    expect(qwenModel?.thinkingLevelMap).toEqual(
      piThinkingProfiles.qwen38.map,
    );

    const mimoPreset = piProviderPresets.find(
      (preset) => preset.name === "Xiaomi MiMo",
    );
    expect(mimoPreset).toBeDefined();
    const mimoModel = mimoPreset!.settingsConfig.models.find(
      (m) => m.id === "mimo-v2.6-pro",
    );
    expect(mimoModel?.thinkingLevelMap).toEqual(
      piThinkingProfiles.mimo.map,
    );
  });

  it("preserves off capability for Sonnet 5.5 and Responses models", () => {
    const sonnetBinding = piThinkingBindings.find(
      (b) => b.catalogKey === "anthropic/claude-sonnet-5.5",
    );
    expect(sonnetBinding).toBeDefined();
    const sonnetProfile = piThinkingProfiles[sonnetBinding!.profileId];
    const sonnetMap = sonnetProfile.map as PiThinkingLevelMap;
    expect(sonnetMap.off).not.toBeNull();

    const gpt6SolBinding = piThinkingBindings.find(
      (b) => b.catalogKey === "openai/gpt-6.1-sol",
    );
    expect(gpt6SolBinding).toBeDefined();
    const gpt6SolProfile = piThinkingProfiles[gpt6SolBinding!.profileId];
    const gpt6SolMap = gpt6SolProfile.map as PiThinkingLevelMap;
    expect(gpt6SolMap.off).toBe("none");
  });
});
