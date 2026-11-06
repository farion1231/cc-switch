import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { dshProviderPresets } from "@/config/dshProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { kimiProviderPresets } from "@/config/kimiProviderPresets";
import { mcodeProviderPresets } from "@/config/mcodeProviderPresets";
import { ompProviderPresets } from "@/config/ompProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { piProviderPresets } from "@/config/piProviderPresets";

const ids = [
  "kimi-k2.7-code",
  "qwen3.7-plus",
  "glm-5.2",
  "qwen3.8-27b",
  "qwen3.8-max",
  "qwen3.8-flash",
  "kimi-k3",
  "deepseek-v4.1-flash",
  "glm-5.3",
  "glm-5.3-flash",
];
const find = <T extends { name: string }>(items: T[]) =>
  items.find((item) => item.name === "Genesis");

describe("Genesis app compatibility", () => {
  it("uses native Responses for Codex and exposes the chat model catalog", () => {
    const preset = find(codexProviderPresets);
    expect(preset?.config).toContain(
      'base_url = "https://token.moi.matrixorigin.cn/v1"',
    );
    expect(preset?.config).toContain('wire_api = "responses"');
    expect(preset?.modelCatalog?.map((model) => model.model)).toEqual(ids);
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

  it("offers every chat model to OpenCode, OpenClaw, Hermes and Pi", () => {
    const opencode = find(opencodeProviderPresets);
    expect(opencode?.settingsConfig.npm).toBe("@ai-sdk/openai-compatible");
    expect(opencode?.settingsConfig.options.baseURL).toBe(
      "https://token.moi.matrixorigin.cn/v1",
    );
    expect(Object.keys(opencode?.settingsConfig.models ?? {})).toEqual(ids);

    const openclaw = find(openclawProviderPresets);
    expect(openclaw?.settingsConfig).toMatchObject({
      baseUrl: "https://token.moi.matrixorigin.cn/v1",
      api: "openai-completions",
    });
    expect(openclaw?.settingsConfig.models?.map((model) => model.id)).toEqual(
      ids,
    );

    const hermes = find(hermesProviderPresets);
    expect(hermes?.settingsConfig).toMatchObject({
      base_url: "https://token.moi.matrixorigin.cn/v1",
      api_mode: "chat_completions",
    });
    expect(hermes?.settingsConfig.models?.map((model) => model.id)).toEqual(
      ids,
    );

    const pi = find(piProviderPresets);
    expect(pi?.settingsConfig).toMatchObject({
      baseUrl: "https://token.moi.matrixorigin.cn/v1",
      api: "openai-completions",
    });
    expect(pi?.settingsConfig.models.map((model) => model.id)).toEqual(ids);
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
