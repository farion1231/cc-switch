import { describe, expect, it } from "vitest";
import {
  qoderProviderPresets,
  qoderAccountPreset,
  QODER_ACCOUNT_PRESET_ID,
} from "@/config/qoderProviderPresets";
import { piProviderPresets } from "@/config/piProviderPresets";
import { getVisiblePresetRows } from "@/components/providers/forms/ProviderPresetSelector";
import {
  presetGroup,
  loginAccountKey,
} from "@/components/providers/forms/presetGroups";

const find = (name: string) =>
  qoderProviderPresets.find((preset) => preset.name === name)!;

describe("Qoder provider presets", () => {
  it.each(["qoder", "qoder-cn"] as const)(
    "offers the native %s account in the login category",
    (app) => {
      const preset = qoderAccountPreset(app);
      expect(preset.providerKey).toBe(QODER_ACCOUNT_PRESET_ID);
      expect(presetGroup(preset)).toBe("login");
      expect(loginAccountKey(app, preset)).toBe("qoder");
      expect(new URL(preset.websiteUrl).hostname).toBe(
        app === "qoder" ? "qoder.com" : "qoder.com.cn",
      );
    },
  );
  it("offers vendors, platforms, cloud and local services with grouped regional variants", () => {
    expect(new Set(qoderProviderPresets.map(presetGroup))).toEqual(
      new Set(["vendor", "thirdparty", "cloud", "local"]),
    );
    const rows = getVisiblePresetRows(
      qoderProviderPresets.map((preset) => ({
        id: preset.providerKey,
        preset,
      })),
      { query: "", t: (key: string) => key },
    );
    expect(
      rows.find(({ row }) => row.family === "kimi")?.row.versions,
    ).toHaveLength(4);
    expect(find("OpenRouter").settingsConfig).toMatchObject({
      protocol: "anthropic",
      authType: "api-key",
      baseUrl: "https://openrouter.ai/api",
    });
    expect(find("RightCode").settingsConfig).toMatchObject({
      protocol: "openai-responses",
      authType: "bearer",
    });
    expect(find("EveryAPI")).toBeUndefined();
    expect(find("Ollama").settingsConfig).toMatchObject({
      protocol: "openai",
      baseUrl: "http://localhost:11434/v1",
      apiKey: "",
    });
  });

  it("excludes unsupported protocols and required custom request headers", () => {
    expect(
      new Set(
        qoderProviderPresets.map((preset) => preset.settingsConfig.protocol),
      ),
    ).toEqual(new Set(["openai", "openai-responses", "anthropic"]));
    expect(find("AWS Bedrock")).toBeUndefined();
    expect(find("NekoCode")).toBeUndefined();
  });

  it("retains model limits and capabilities using the native Qoder schema", () => {
    const keys = new Set<string>();
    for (const preset of qoderProviderPresets) {
      expect(keys.has(preset.providerKey)).toBe(false);
      keys.add(preset.providerKey);
      const config = preset.settingsConfig;
      const source = piProviderPresets.find(
        (entry) => entry.providerKey === preset.providerKey,
      )!;
      expect(config.models!.map((model) => model.model)).toEqual(
        source.settingsConfig.models.map((model) => model.id),
      );
      expect(config.model).toBe(config.models![0].model);
      expect(config).not.toHaveProperty("api");
      expect(config).not.toHaveProperty("headers");
      for (const [index, model] of config.models!.entries()) {
        const original = source.settingsConfig.models[index];
        expect(model.contextWindow).toBe(original.contextWindow);
        expect(model.maxOutputTokens).toBe(original.maxTokens);
        expect(model.capabilities?.vision).toBe(
          original.input.includes("image"),
        );
        expect(model.capabilities?.thinking?.modes.length !== 0).toBe(
          original.reasoning,
        );
        for (const field of [
          "id",
          "name",
          "input",
          "maxTokens",
          "compat",
          "thinkingLevelMap",
        ])
          expect(model).not.toHaveProperty(field);
      }
    }
  });
});
