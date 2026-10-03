import { describe, expect, it } from "vitest";

import {
  CLAUDE_DESKTOP_ROLE_ROUTE_IDS,
  claudeDesktopProviderPresets,
} from "./claudeDesktopProviderPresets";
import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";
import { hermesProviderPresets } from "./hermesProviderPresets";
import { openclawProviderPresets } from "./openclawProviderPresets";
import { opencodeProviderPresets } from "./opencodeProviderPresets";

const presetName = "Cheaper Inference";
const allPresetGroups = [
  ["Claude Code", providerPresets],
  ["Claude Desktop", claudeDesktopProviderPresets],
  ["Codex", codexProviderPresets],
  ["OpenCode", opencodeProviderPresets],
  ["OpenClaw", openclawProviderPresets],
  ["Hermes", hermesProviderPresets],
] as const;

const anthropicBaseUrl = "https://api.cheaperinference.com";
const openAiBaseUrl = "https://api.cheaperinference.com/v1";
const brandDetails = {
  websiteUrl: "https://cheaperinference.com",
  apiKeyUrl: "https://cheaperinference.com/signup",
  category: "aggregator",
};

function findEntry<T extends { name: string }>(entries: readonly T[]) {
  return entries.find((entry) => entry.name === presetName);
}

describe("Cheaper Inference provider presets", () => {
  it.each(allPresetGroups)(
    "%s registers exactly one Cheaper Inference preset",
    (_surface, entries) => {
      expect(entries.filter((entry) => entry.name === presetName)).toHaveLength(
        1,
      );
    },
  );

  it("configures Claude Code with the Anthropic endpoint", () => {
    expect(findEntry(providerPresets)).toMatchObject({
      ...brandDetails,
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: anthropicBaseUrl,
          ANTHROPIC_AUTH_TOKEN: "",
          ANTHROPIC_MODEL: "claude-sonnet-5",
          ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-sonnet-5",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-5",
          ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-sonnet-5",
        },
      },
      endpointCandidates: [anthropicBaseUrl],
    });
  });

  it("configures Claude Desktop with a direct Sonnet route", () => {
    expect(findEntry(claudeDesktopProviderPresets)).toMatchObject({
      ...brandDetails,
      baseUrl: anthropicBaseUrl,
      mode: "direct",
      apiFormat: "anthropic",
      modelRoutes: [
        {
          routeId: CLAUDE_DESKTOP_ROLE_ROUTE_IDS.sonnet,
          upstreamModel: "claude-sonnet-5",
          supports1m: false,
        },
      ],
      endpointCandidates: [anthropicBaseUrl],
    });
  });

  it("configures Codex with the OpenAI-compatible endpoint", () => {
    const preset = findEntry(codexProviderPresets)!;
    expect(preset).toMatchObject({
      ...brandDetails,
      auth: { OPENAI_API_KEY: "" },
      endpointCandidates: [openAiBaseUrl],
    });
    expect(preset.config).toContain('model = "gpt-5.4"');
    expect(preset.config).toContain(`base_url = "${openAiBaseUrl}"`);
  });

  it("configures OpenCode with the OpenAI-compatible endpoint", () => {
    const preset = findEntry(opencodeProviderPresets)!;
    expect(preset).toMatchObject({
      ...brandDetails,
      settingsConfig: {
        npm: "@ai-sdk/openai-compatible",
        name: presetName,
        options: { baseURL: openAiBaseUrl, apiKey: "" },
        models: {
          "claude-sonnet-5": {
            limit: { context: 1000000, output: 64000 },
          },
        },
      },
    });
    expect(Object.keys(preset.settingsConfig.models)).toEqual([
      "gpt-5.4",
      "gpt-5.4-mini",
      "claude-sonnet-5",
    ]);
  });

  it("configures OpenClaw with the OpenAI-compatible endpoint", () => {
    expect(findEntry(openclawProviderPresets)).toMatchObject({
      ...brandDetails,
      settingsConfig: {
        baseUrl: openAiBaseUrl,
        apiKey: "",
        api: "openai-completions",
        models: [
          { id: "gpt-5.4", contextWindow: 1000000 },
          { id: "gpt-5.4-mini", contextWindow: 400000 },
        ],
      },
      suggestedDefaults: {
        model: { primary: "cheaperinference/gpt-5.4" },
      },
    });
  });

  it("configures Hermes with the OpenAI-compatible endpoint", () => {
    expect(findEntry(hermesProviderPresets)).toMatchObject({
      ...brandDetails,
      settingsConfig: {
        name: "cheaperinference",
        base_url: openAiBaseUrl,
        api_key: "",
        api_mode: "chat_completions",
        models: [
          { id: "gpt-5.4", context_length: 1000000 },
          { id: "gpt-5.4-mini", context_length: 400000 },
        ],
      },
      suggestedDefaults: {
        model: { default: "gpt-5.4", provider: "cheaperinference" },
      },
    });
  });
});
