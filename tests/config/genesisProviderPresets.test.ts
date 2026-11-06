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

// Presets need one immediately usable model; the current catalog is fetched
// with the account's API key instead of being frozen in the app bundle.
const starterModels = ["qwen3.8-flash"];
const find = <T extends { name: string }>(items: T[]) =>
  items.find((item) => item.name === "Genesis");

describe("Genesis app compatibility", () => {
  it("links every Genesis preset to the current credential page", () => {
    const catalogs = [
      providerPresets,
      claudeDesktopProviderPresets,
      codexProviderPresets,
      opencodeProviderPresets,
      openclawProviderPresets,
      hermesProviderPresets,
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

  it("uses native Responses for Codex with a starter model", () => {
    const preset = find(codexProviderPresets);
    expect(preset?.config).toContain(
      'base_url = "https://token.moi.matrixorigin.cn/v1"',
    );
    expect(preset?.config).toContain('wire_api = "responses"');
    expect(preset?.modelCatalog?.map((model) => model.model)).toEqual(
      starterModels,
    );
  });

  it("routes Claude Desktop through Chat Completions", () => {
    const preset = find(claudeDesktopProviderPresets);
    expect(preset).toMatchObject({
      mode: "proxy",
      apiFormat: "openai_chat",
      baseUrl: "https://token.moi.matrixorigin.cn",
    });
    expect(preset?.modelRoutes?.length).toBeGreaterThan(0);
  });

  it("seeds OpenCode, OpenClaw, Hermes and Pi with one model", () => {
    const opencode = find(opencodeProviderPresets);
    expect(opencode?.settingsConfig.npm).toBe("@ai-sdk/openai-compatible");
    expect(opencode?.settingsConfig.options.baseURL).toBe(
      "https://token.moi.matrixorigin.cn/v1",
    );
    expect(Object.keys(opencode?.settingsConfig.models ?? {})).toEqual(
      starterModels,
    );

    const openclaw = find(openclawProviderPresets);
    expect(openclaw?.settingsConfig).toMatchObject({
      baseUrl: "https://token.moi.matrixorigin.cn/v1",
      api: "openai-completions",
    });
    expect(openclaw?.settingsConfig.models?.map((model) => model.id)).toEqual(
      starterModels,
    );

    const hermes = find(hermesProviderPresets);
    expect(hermes?.settingsConfig).toMatchObject({
      base_url: "https://token.moi.matrixorigin.cn/v1",
      api_mode: "chat_completions",
    });
    expect(hermes?.settingsConfig.models?.map((model) => model.id)).toEqual(
      starterModels,
    );

    const pi = find(piProviderPresets);
    expect(pi?.settingsConfig).toMatchObject({
      baseUrl: "https://token.moi.matrixorigin.cn/v1",
      api: "openai-completions",
    });
    expect(pi?.settingsConfig.models.map((model) => model.id)).toEqual(
      starterModels,
    );
  });

  it("inherits the Pi preset in compatible Pi based clients", () => {
    for (const presets of [
      kimiProviderPresets,
      mcodeProviderPresets,
      dshProviderPresets,
      ompProviderPresets,
    ]) {
      expect(presets.some((preset) => preset.name === "Genesis")).toBe(true);
    }
  });
});
