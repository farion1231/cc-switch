import { describe, expect, it } from "vitest";

import { claudeDesktopProviderPresets } from "./claudeDesktopProviderPresets";
import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";
import { geminiProviderPresets } from "./geminiProviderPresets";
import { hermesProviderPresets } from "./hermesProviderPresets";
import {
  openclawProviderPresets,
  rebaseOpenClawSuggestedDefaults,
} from "./openclawProviderPresets";
import { piModelCatalog } from "./piModelCatalog";
import { piProviderPresets } from "./piProviderPresets";

const laonongPresetGroups = [
  ["Claude Code", providerPresets],
  ["Claude Desktop", claudeDesktopProviderPresets],
  ["Codex", codexProviderPresets],
  ["Gemini", geminiProviderPresets],
  ["Hermes", hermesProviderPresets],
  ["OpenClaw", openclawProviderPresets],
  ["Pi", piProviderPresets],
] as const;

const OPENAI_BASE_URL = "https://api.laonongapi.com/v1";
const ANTHROPIC_BASE_URL = "https://api.laonongapi.com";
const DEFAULT_MODEL_ID = "gpt-5.6-luna";
const DEFAULT_MODEL_NAME = "GPT-5.6 Luna";

function findLaonongEntry<T extends { name: string }>(entries: readonly T[]) {
  return entries.find((entry) => entry.name === "LaonongAPI");
}

describe("LaonongAPI provider presets", () => {
  it.each(laonongPresetGroups)(
    "%s registers exactly one LaonongAPI preset",
    (_surface, entries) => {
      expect(
        entries.filter((entry) => entry.name === "LaonongAPI"),
      ).toHaveLength(1);
    },
  );

  it("uses the OpenAI-compatible base URL on OpenAI surfaces", () => {
    const codex = findLaonongEntry(codexProviderPresets)!;
    expect(codex.config).toContain(`base_url = "${OPENAI_BASE_URL}"`);

    const hermes = findLaonongEntry(hermesProviderPresets)!;
    expect(hermes.settingsConfig.base_url).toBe(OPENAI_BASE_URL);

    const openclaw = findLaonongEntry(openclawProviderPresets)!;
    expect(openclaw.settingsConfig.baseUrl).toBe(OPENAI_BASE_URL);

    const pi = findLaonongEntry(piProviderPresets)!;
    expect(pi.settingsConfig.baseUrl).toBe(OPENAI_BASE_URL);
  });

  it("uses the Anthropic/Gemini base URL on those surfaces", () => {
    const claude = findLaonongEntry(providerPresets)!;
    expect(claude).toMatchObject({
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: ANTHROPIC_BASE_URL,
        },
      },
      endpointCandidates: [ANTHROPIC_BASE_URL],
    });

    const desktop = findLaonongEntry(claudeDesktopProviderPresets)!;
    expect(desktop.baseUrl).toBe(ANTHROPIC_BASE_URL);

    const gemini = findLaonongEntry(geminiProviderPresets)!;
    expect(gemini).toMatchObject({
      baseURL: ANTHROPIC_BASE_URL,
      model: "gemini-3.1-flash-lite",
      settingsConfig: {
        env: {
          GOOGLE_GEMINI_BASE_URL: ANTHROPIC_BASE_URL,
          GEMINI_API_KEY: "",
          GEMINI_MODEL: "gemini-3.1-flash-lite",
        },
      },
    });
  });

  it("configures Codex for Chat Completions conversion", () => {
    const preset = findLaonongEntry(codexProviderPresets)!;
    expect(preset).toMatchObject({
      apiFormat: "openai_chat",
      modelCatalog: [
        {
          model: DEFAULT_MODEL_ID,
          displayName: DEFAULT_MODEL_NAME,
        },
      ],
    });
    expect(preset.config).toContain(`model = "${DEFAULT_MODEL_ID}"`);
    expect(preset.config).toContain('wire_api = "responses"');
  });

  it("uses bare upstream model IDs on OpenAI-compatible surfaces", () => {
    const hermes = findLaonongEntry(hermesProviderPresets)!;
    expect(hermes.settingsConfig.models).toEqual([
      { id: DEFAULT_MODEL_ID, name: DEFAULT_MODEL_NAME },
    ]);
    expect(hermes.suggestedDefaults).toMatchObject({
      model: { default: DEFAULT_MODEL_ID, provider: "laonongapi" },
    });

    const openclaw = findLaonongEntry(openclawProviderPresets)!;
    expect(openclaw.settingsConfig.models?.map((model) => model.id)).toEqual([
      DEFAULT_MODEL_ID,
    ]);
    expect(openclaw.suggestedDefaults).toMatchObject({
      model: { primary: `laonongapi/${DEFAULT_MODEL_ID}` },
      modelCatalog: {
        [`laonongapi/${DEFAULT_MODEL_ID}`]: { alias: DEFAULT_MODEL_NAME },
      },
    });

    const pi = findLaonongEntry(piProviderPresets)!;
    expect(pi.settingsConfig.models.map((model) => model.id)).toEqual([
      "gpt-5.6-luna",
      "claude-sonnet-5",
      "gemini-3.1-flash-lite",
      "deepseek-v4-flash",
    ]);
  });

  it("keeps OpenClaw defaults rebased to the submitted provider key", () => {
    const preset = findLaonongEntry(openclawProviderPresets)!;
    const rebased = rebaseOpenClawSuggestedDefaults(
      preset.suggestedDefaults!,
      "my-laonong",
    );
    expect(rebased.model?.primary).toBe("my-laonong/gpt-5.6-luna");
    expect(rebased.modelCatalog).toHaveProperty("my-laonong/gpt-5.6-luna");
  });

  it("backs every Pi catalog key with capability data", () => {
    for (const key of [
      "openai/gpt-5.6-luna",
      "anthropic/claude-sonnet-5",
      "google/gemini-3.1-flash-lite",
      "deepseek/deepseek-v4-flash",
    ]) {
      expect(piModelCatalog).toHaveProperty(key);
    }
  });

  it("keeps Anthropic route models and Gemini brand icon", () => {
    const claude = findLaonongEntry(providerPresets)!;
    expect(claude).toMatchObject({
      settingsConfig: {
        env: {
          ANTHROPIC_MODEL: "anthropic/claude-opus-5",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "anthropic/claude-sonnet-5",
          ANTHROPIC_DEFAULT_HAIKU_MODEL: "anthropic/claude-haiku-4.5",
        },
      },
    });

    const desktop = findLaonongEntry(claudeDesktopProviderPresets)!;
    expect(desktop).toMatchObject({
      apiFormat: "anthropic",
      mode: "proxy",
      baseUrl: ANTHROPIC_BASE_URL,
    });

    const gemini = findLaonongEntry(geminiProviderPresets)!;
    expect(gemini.icon).toBe("laonongapi");
    expect(gemini.iconColor).toBe("#7B61FF");
  });
});
