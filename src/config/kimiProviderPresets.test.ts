import { describe, expect, it } from "vitest";
import {
  KIMI_CAPABILITIES,
  kimiConfigSchema,
  kimiProviderPresets,
  kimiAccountPreset,
  KIMI_ACCOUNT_PRESET_ID,
} from "./kimiProviderPresets";
import { piProviderPresets } from "./piProviderPresets";
import { getVisiblePresetRows } from "@/components/providers/forms/ProviderPresetSelector";
import {
  presetGroup,
  presetRowGroup,
  presetRowName,
} from "@/components/providers/forms/presetGroups";
import type { TFunction } from "i18next";

const find = (name: string) =>
  kimiProviderPresets.find((preset) => preset.name === name)!;
const model = (name: string, id: string) => {
  const preset = find(name);
  return preset.settingsConfig.models[`${preset.providerKey}/${id}`];
};

describe("kimiProviderPresets", () => {
  it("keeps official account login separate from the API-key vendor family", () => {
    const t = ((key: string) => key) as TFunction;
    const rows = getVisiblePresetRows(
      [
        { id: KIMI_ACCOUNT_PRESET_ID, preset: kimiAccountPreset },
        ...kimiProviderPresets.map((preset) => ({
          id: preset.providerKey,
          preset,
        })),
      ],
      { query: "Kimi", t },
    );
    const login = rows.find(
      ({ row }) => row.key === KIMI_ACCOUNT_PRESET_ID,
    )!.row;
    expect(presetRowGroup(login)).toBe("login");
    expect(login.family).toBeUndefined();
    expect(
      rows.find(({ row }) => row.family === "kimi")!.row.versions,
    ).toHaveLength(4);
  });
  it("provides the compatible catalog across vendors, platforms, cloud and local groups", () => {
    const compatible = piProviderPresets.filter(
      (preset) => preset.settingsConfig.api !== "bedrock-converse-stream",
    );
    expect(kimiProviderPresets.map((preset) => preset.name)).toEqual(
      compatible.map((preset) => preset.name),
    );
    expect(new Set(kimiProviderPresets.map(presetGroup))).toEqual(
      new Set(["vendor", "thirdparty", "cloud", "local"]),
    );
    for (const name of [
      "88API",
      "A6API",
      "AICodeMirror",
      "OpenRouter",
      "TokenFactory",
    ]) {
      expect(presetGroup(find(name))).toBe("thirdparty");
      expect(find(name).apiKeyUrl).toBeTruthy();
    }
    expect(find("Third-party platform")).toBeUndefined();
  });

  it("keeps Kimi as one vendor row with its pay-as-you-go, coding and regional variants", () => {
    const entries = kimiProviderPresets.map((preset, index) => ({
      id: String(index),
      preset,
    }));
    const t = ((key: string) => key) as TFunction;
    const rows = getVisiblePresetRows(entries, { query: "", t });
    const kimi = rows.find(({ row }) => row.family === "kimi")!.row;
    expect(presetRowGroup(kimi)).toBe("vendor");
    expect(presetRowName(kimi, t)).toBe("Kimi");
    expect(kimi.versions.map((entry) => entry.preset.name)).toEqual([
      "Kimi",
      "Kimi Global",
      "Kimi For Coding",
      "Kimi For Coding Global",
    ]);
    expect(find("Kimi Code")).toBeUndefined();
    expect(find("Moonshot (CN)")).toBeUndefined();
    expect(find("Moonshot (Global)")).toBeUndefined();
    expect(find("Kimi").settingsConfig.provider).toMatchObject({
      type: "kimi",
      base_url: "https://api.moonshot.cn/v1",
    });
    expect(find("Kimi For Coding").settingsConfig.provider).toMatchObject({
      type: "kimi",
      base_url: "https://api.kimi.com/coding/v1",
    });
    expect(model("Kimi", "kimi-k3")).toMatchObject({
      max_context_size: 1048576,
      support_efforts: ["low", "high", "max"],
      default_effort: "high",
    });
    expect(model("Kimi", "kimi-k3").capabilities).toEqual(
      expect.arrayContaining([
        "image_in",
        "thinking",
        "always_thinking",
        "dynamically_loaded_tools",
      ]),
    );
  });

  it("uses cloud providers' real protocols and excludes Bedrock's unsupported signing protocol", () => {
    for (const name of [
      "Ollama Cloud",
      "SambaNova",
      "DigitalOcean",
      "Microsoft Foundry",
    ]) {
      expect(presetGroup(find(name))).toBe("cloud");
    }
    expect(find("Microsoft Foundry").settingsConfig.provider).toMatchObject({
      type: "anthropic",
      base_url: "https://YOUR_RESOURCE_NAME.services.ai.azure.com/anthropic",
    });
    expect(find("AWS Bedrock")).toBeUndefined();
    expect(find("OpenRouter").settingsConfig.provider).toMatchObject({
      type: "anthropic",
      base_url: "https://openrouter.ai/api",
    });
  });

  it.each([
    ["Ollama", "http://localhost:11434/v1"],
    ["LM Studio", "http://localhost:1234/v1"],
    ["vLLM", "http://localhost:8000/v1"],
  ])(
    "configures %s without requiring a secret for its unauthenticated default server",
    (name, baseUrl) => {
      expect(find(name).category).toBe("local");
      expect(find(name).settingsConfig.provider).toMatchObject({
        type: "openai",
        base_url: baseUrl,
        api_key: "local",
      });
    },
  );

  it("writes only Kimi-native model fields while retaining reasoning and image capabilities", () => {
    const aliases = new Set<string>();
    for (const preset of kimiProviderPresets) {
      expect(
        kimiConfigSchema.safeParse(preset.settingsConfig).success,
        preset.name,
      ).toBe(true);
      const source = piProviderPresets.find(
        (entry) => entry.providerKey === preset.providerKey,
      )!;
      expect(preset.settingsConfig).not.toHaveProperty("api");
      expect(preset.settingsConfig).not.toHaveProperty("compat");
      for (const [alias, config] of Object.entries(
        preset.settingsConfig.models,
      )) {
        expect(aliases.has(alias), alias).toBe(false);
        aliases.add(alias);
        const original = source.settingsConfig.models.find(
          (entry) => entry.id === config.model,
        )!;
        expect(config.max_context_size).toBe(original.contextWindow);
        expect(config.capabilities?.includes("image_in")).toBe(
          original.input.includes("image"),
        );
        expect(config.capabilities?.includes("thinking")).toBe(
          original.reasoning,
        );
        for (const capability of config.capabilities ?? []) {
          expect(KIMI_CAPABILITIES).toContain(capability);
        }
        expect(config).not.toHaveProperty("compat");
        expect(config).not.toHaveProperty("thinkingLevelMap");
        expect(config).not.toHaveProperty("input");
        if (Array.isArray(config.support_efforts))
          expect(config.support_efforts).toContain(config.default_effort);
      }
    }
    expect(model("PackyCode", "claude-opus-5-5")).toMatchObject({
      adaptive_thinking: true,
      max_output_size: 128000,
    });
  });
});
