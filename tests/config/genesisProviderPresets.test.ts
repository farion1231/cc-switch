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

// Optional catalogs stay empty; clients requiring a model start with K3.
const emptyModels: string[] = [];
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

  it("uses native Responses for Codex without a pinned model", () => {
    const preset = find(codexProviderPresets);
    expect(preset?.config).toContain(
      'base_url = "https://token.moi.matrixorigin.cn/v1"',
    );
    expect(preset?.config).toContain('wire_api = "responses"');
    expect(preset?.config).not.toMatch(/^model\s*=/m);
    expect(preset?.modelCatalog?.map((model) => model.model)).toEqual(
      emptyModels,
    );
  });

  it("routes Claude Desktop through Chat Completions", () => {
    const preset = find(claudeDesktopProviderPresets);
    expect(preset).toMatchObject({
      mode: "proxy",
      apiFormat: "openai_chat",
      baseUrl: "https://token.moi.matrixorigin.cn",
    });
    expect(preset?.modelRoutes?.map((route) => route.upstreamModel)).toEqual([
      "kimi-k3",
    ]);
  });

  it("leaves optional model lists empty and gives Pi its required starter", () => {
    const opencode = find(opencodeProviderPresets);
    expect(opencode?.settingsConfig.npm).toBe("@ai-sdk/openai-compatible");
    expect(opencode?.settingsConfig.options.baseURL).toBe(
      "https://token.moi.matrixorigin.cn/v1",
    );
    expect(Object.keys(opencode?.settingsConfig.models ?? {})).toEqual(
      emptyModels,
    );

    const openclaw = find(openclawProviderPresets);
    expect(openclaw?.settingsConfig).toMatchObject({
      baseUrl: "https://token.moi.matrixorigin.cn/v1",
      api: "openai-completions",
    });
    expect(openclaw?.settingsConfig.models?.map((model) => model.id)).toEqual(
      emptyModels,
    );
    expect(openclaw?.suggestedDefaults?.model).toBeUndefined();

    const hermes = find(hermesProviderPresets);
    expect(hermes?.settingsConfig).toMatchObject({
      base_url: "https://token.moi.matrixorigin.cn/v1",
      api_mode: "chat_completions",
    });
    expect(hermes?.settingsConfig.models?.map((model) => model.id)).toEqual(
      emptyModels,
    );
    expect(hermes?.suggestedDefaults?.model).toBeUndefined();

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
