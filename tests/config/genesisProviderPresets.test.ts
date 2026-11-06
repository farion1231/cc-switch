import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { dshProviderPresets } from "@/config/dshProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { kimiProviderPresets } from "@/config/kimiProviderPresets";
import { mcodeProviderPresets } from "@/config/mcodeProviderPresets";
import { ompProviderPresets } from "@/config/ompProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { piProviderPresets } from "@/config/piProviderPresets";

const find = <T extends { name: string }>(items: T[]) =>
  items.find((item) => item.name === "Genesis");

describe("Genesis app compatibility", () => {
  it("links every Genesis preset to the current credential page", () => {
    const catalogs = [
      providerPresets,
      codexProviderPresets,
      piProviderPresets,
      kimiProviderPresets,
      mcodeProviderPresets,
      dshProviderPresets,
      ompProviderPresets,
    ];
    for (const catalog of catalogs) {
      expect(
        catalog.find((preset) => preset.name === "Genesis")?.apiKeyUrl,
      ).toBe("https://uc.moi.matrixorigin.cn/account/credentials?app=genesis");
    }
  });

  it("does not advertise unverified client presets", () => {
    for (const catalog of [
      claudeDesktopProviderPresets,
      opencodeProviderPresets,
      openclawProviderPresets,
      hermesProviderPresets,
    ]) {
      expect(catalog.some((preset) => preset.name === "Genesis")).toBe(false);
    }
  });

  it("uses native Responses for Codex with a K3 starter", () => {
    const preset = find(codexProviderPresets);
    expect(preset?.config).toContain(
      'base_url = "https://token.moi.matrixorigin.cn/v1"',
    );
    expect(preset?.config).toContain('wire_api = "responses"');
    expect(preset?.config).toMatch(/^model\s*=\s*"kimi-k3"$/m);
    expect(preset?.modelCatalog?.map((model) => model.model)).toEqual([
      "kimi-k3",
    ]);
  });

  it("gives Pi its required K3 starter", () => {
    const pi = find(piProviderPresets);
    expect(pi?.settingsConfig).toMatchObject({
      baseUrl: "https://token.moi.matrixorigin.cn/v1",
      api: "openai-completions",
    });
    expect(pi?.settingsConfig.models.map((model) => model.id)).toEqual([
      "kimi-k3",
    ]);
  });

  it("inherits the K3 starter in compatible Pi based clients", () => {
    expect(
      Object.keys(find(kimiProviderPresets)?.settingsConfig.models ?? {}),
    ).toEqual(["cc-switch-genesis/kimi-k3"]);
    for (const presets of [mcodeProviderPresets, dshProviderPresets]) {
      expect(Object.keys(find(presets)?.settingsConfig.models ?? {})).toEqual([
        "kimi-k3",
      ]);
    }
    expect(
      find(ompProviderPresets)?.settingsConfig.models.map((model) => model.id),
    ).toEqual(["kimi-k3"]);
  });
});
