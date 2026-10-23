import { describe, expect, it } from "vitest";
import {
  QWEN_AUTH_TYPES,
  QWEN_EDITABLE_AUTH_TYPES,
  qwenConfigSchema,
  qwenProviderPresets,
} from "./qwenProviderPresets";
import { piProviderPresets } from "./piProviderPresets";
import { getVisiblePresetRows } from "@/components/providers/forms/ProviderPresetSelector";
import {
  presetGroup,
  presetRowGroup,
} from "@/components/providers/forms/presetGroups";
import type { TFunction } from "i18next";

const find = (name: string) =>
  qwenProviderPresets.find((preset) => preset.name === name)!;

describe("qwenProviderPresets", () => {
  it("provides the compatible catalog across vendors, platforms, cloud and local groups", () => {
    const compatible = piProviderPresets.filter(
      (preset) =>
        preset.settingsConfig.api !== "bedrock-converse-stream" &&
        !preset.settingsConfig.compat &&
        preset.settingsConfig.models.length > 0,
    );
    expect(qwenProviderPresets.map((preset) => preset.name)).toEqual(
      compatible.map((preset) => preset.name),
    );
    expect(new Set(qwenProviderPresets.map(presetGroup))).toEqual(
      new Set(["vendor", "thirdparty", "cloud", "local"]),
    );
    expect(find("AWS Bedrock")).toBeUndefined();
  });

  it("keeps QwenCloud as one vendor row with its plan variants", () => {
    const t = ((key: string) => key) as TFunction;
    const rows = getVisiblePresetRows(
      qwenProviderPresets.map((preset, index) => ({
        id: String(index),
        preset,
      })),
      { query: "", t },
    );
    const qwen = rows.find(({ row }) => row.family === "qwencloud")!.row;
    expect(presetRowGroup(qwen)).toBe("vendor");
    expect(qwen.versions.map((entry) => entry.preset.name)).toEqual([
      "QwenCloud",
      "QwenCloud For Coding",
      "QwenCloud Token Plan",
    ]);
    expect(find("QwenCloud For Coding").settingsConfig).toMatchObject({
      authType: "anthropic",
      baseUrl: "https://coding-intl.dashscope.aliyuncs.com/apps/anthropic",
    });
    expect(find("QwenCloud").settingsConfig).toMatchObject({
      authType: "openai",
      baseUrl: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
    });
  });

  it("does not offer the Gemini protocol for new providers or presets", () => {
    expect(QWEN_EDITABLE_AUTH_TYPES).not.toContain("gemini");
    // Imported native entries may still use it.
    expect(QWEN_AUTH_TYPES).toContain("gemini");
    expect(
      qwenProviderPresets.filter(
        (preset) => preset.settingsConfig.authType === "gemini",
      ),
    ).toEqual([]);
  });

  it("strips the cc-switch- prefix into a valid native provider key", () => {
    const keys = new Set<string>();
    for (const preset of qwenProviderPresets) {
      expect(preset.providerKey).not.toMatch(/^cc-switch-/);
      expect(preset.providerKey).toMatch(/^[a-z0-9]+(-[a-z0-9]+)*$/);
      expect(keys.has(preset.providerKey), preset.providerKey).toBe(false);
      keys.add(preset.providerKey);
    }
    expect(find("QwenCloud").providerKey).toBe("qwencloud");
  });

  it.each([
    ["Ollama", "http://localhost:11434/v1"],
    ["LM Studio", "http://localhost:1234/v1"],
    ["vLLM", "http://localhost:8000/v1"],
  ])(
    "configures %s without requiring a secret for its unauthenticated default server",
    (name, baseUrl) => {
      expect(find(name).category).toBe("local");
      expect(find(name).settingsConfig).toMatchObject({
        authType: "openai",
        baseUrl,
        apiKey: "local",
      });
    },
  );

  it("writes only Qwen-native fields with the model context window", () => {
    for (const preset of qwenProviderPresets) {
      expect(
        qwenConfigSchema.safeParse(preset.settingsConfig).success,
        preset.name,
      ).toBe(true);
      expect(QWEN_AUTH_TYPES).toContain(preset.settingsConfig.authType);
      expect(preset.settingsConfig).not.toHaveProperty("api");
      expect(preset.settingsConfig).not.toHaveProperty("compat");
      expect(preset.settingsConfig).not.toHaveProperty("headers");
      const source = piProviderPresets.find(
        (entry) =>
          entry.providerKey.replace(/^cc-switch-/, "") === preset.providerKey,
      )!;
      expect(preset.settingsConfig.models).toHaveLength(
        source.settingsConfig.models.length,
      );
      preset.settingsConfig.models.forEach((model, index) => {
        const original = source.settingsConfig.models[index];
        expect(model.id).toBe(original.id);
        expect(model.name).toBe(original.name);
        expect(model.generationConfig?.contextWindowSize).toBe(
          original.contextWindow,
        );
        expect(model).not.toHaveProperty("compat");
        expect(model).not.toHaveProperty("input");
        expect(model).not.toHaveProperty("thinkingLevelMap");
      });
    }
  });

  it("carries required request headers into each model's generation config", () => {
    expect(
      find("NekoCode").settingsConfig.models[0].generationConfig,
    ).toMatchObject({
      customHeaders: { "User-Agent": "claude-cli/2.1.161 (external, cli)" },
    });
  });
});
