import { describe, expect, it } from "vitest";

import { claudeDesktopProviderPresets } from "./claudeDesktopProviderPresets";
import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";
import { hermesProviderPresets } from "./hermesProviderPresets";
import { openclawProviderPresets } from "./openclawProviderPresets";
import { opencodeProviderPresets } from "./opencodeProviderPresets";
import { getIconMetadata, getIconUrl } from "../icons/extracted";

const presetCollections = [
  ["Claude Code", providerPresets],
  ["Claude Desktop", claudeDesktopProviderPresets],
  ["Codex", codexProviderPresets],
  ["OpenCode", opencodeProviderPresets],
  ["OpenClaw", openclawProviderPresets],
  ["Hermes", hermesProviderPresets],
] as const;

const anthropicBaseUrl = "https://api.tokensmarket.ai";
const openAiBaseUrl = `${anthropicBaseUrl}/v1`;
const brandFields = {
  websiteUrl: "https://www.tokensmarket.ai",
  apiKeyUrl: "https://www.tokensmarket.ai/console",
  category: "aggregator",
  icon: "tokensmarket",
};

function getTokenMarketPreset<T extends { name: string }>(
  presets: readonly T[],
) {
  return presets.find((preset) => preset.name === "Token Market");
}

describe("Token Market provider presets", () => {
  it.each(presetCollections)(
    "%s registers exactly one Token Market preset",
    (_name, presets) => {
      expect(
        presets.filter((preset) => preset.name === "Token Market"),
      ).toHaveLength(1);
    },
  );

  it("configures Claude Code with the Anthropic-compatible base", () => {
    expect(getTokenMarketPreset(providerPresets)).toMatchObject({
      ...brandFields,
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: anthropicBaseUrl,
          ANTHROPIC_AUTH_TOKEN: "",
          ANTHROPIC_MODEL: "claude-sonnet-5",
          ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-haiku-4-5",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-5",
          ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-5",
        },
      },
      endpointCandidates: [anthropicBaseUrl],
      modelsUrl: `${openAiBaseUrl}/models`,
    });
  });

  it("configures Claude Desktop with native Anthropic model routes", () => {
    expect(getTokenMarketPreset(claudeDesktopProviderPresets)).toMatchObject({
      ...brandFields,
      baseUrl: anthropicBaseUrl,
      mode: "proxy",
      apiFormat: "anthropic",
      modelRoutes: [
        {
          routeId: "claude-sonnet-5",
          upstreamModel: "claude-sonnet-5",
          supports1m: true,
        },
        {
          routeId: "claude-opus-5",
          upstreamModel: "claude-opus-5",
          supports1m: true,
        },
        {
          routeId: "claude-haiku-4-5",
          upstreamModel: "claude-haiku-4-5",
          supports1m: true,
        },
      ],
      endpointCandidates: [anthropicBaseUrl],
    });
  });

  it("configures Codex with the native Responses API", () => {
    const codex = getTokenMarketPreset(codexProviderPresets)!;
    expect(codex).toMatchObject({
      ...brandFields,
      auth: { OPENAI_API_KEY: "" },
      endpointCandidates: [openAiBaseUrl],
      apiFormat: "openai_responses",
      modelCatalog: [
        {
          model: "gpt-5.6-luna",
          displayName: "GPT-5.6 Luna",
          contextWindow: 1050000,
          inputModalities: ["text", "image"],
        },
      ],
    });
    expect(codex.config).toContain('model = "gpt-5.6-luna"');
    expect(codex.config).toContain(`base_url = "${openAiBaseUrl}"`);
    expect(codex.config).toContain('wire_api = "responses"');
  });

  it("configures OpenCode with an OpenAI-compatible model catalog", () => {
    const opencode = getTokenMarketPreset(opencodeProviderPresets)!;
    expect(opencode).toMatchObject({
      ...brandFields,
      settingsConfig: {
        npm: "@ai-sdk/openai-compatible",
        name: "Token Market",
        options: {
          baseURL: openAiBaseUrl,
          apiKey: "",
          setCacheKey: true,
        },
        models: {
          "claude-sonnet-5": { name: "Claude Sonnet 5" },
          "gpt-5.6-luna": { name: "GPT-5.6 Luna" },
          "gemini-3.5-flash": { name: "Gemini 3.5 Flash" },
        },
      },
    });
    expect(`${opencode.settingsConfig.options.baseURL}/chat/completions`).toBe(
      `${openAiBaseUrl}/chat/completions`,
    );
  });

  it("configures OpenClaw with OpenAI Chat and suggested defaults", () => {
    const openclaw = getTokenMarketPreset(openclawProviderPresets)!;
    expect(openclaw).toMatchObject({
      ...brandFields,
      settingsConfig: {
        baseUrl: openAiBaseUrl,
        apiKey: "",
        api: "openai-completions",
        models: [
          {
            id: "claude-sonnet-5",
            contextWindow: 1000000,
            maxTokens: 128000,
            cost: { input: 0.5, output: 2.5 },
          },
          {
            id: "gpt-5.6-luna",
            contextWindow: 1050000,
            maxTokens: 128000,
            cost: { input: 0.03, output: 0.18 },
          },
          {
            id: "gemini-3.5-flash",
            contextWindow: 1048576,
            maxTokens: 65536,
            cost: { input: 0.225, output: 1.35 },
          },
        ],
      },
      suggestedDefaults: {
        model: {
          primary: "tokensmarket/claude-sonnet-5",
          fallbacks: [
            "tokensmarket/gpt-5.6-luna",
            "tokensmarket/gemini-3.5-flash",
          ],
        },
      },
    });
  });

  it("configures Hermes with OpenAI Chat and explicit context windows", () => {
    expect(getTokenMarketPreset(hermesProviderPresets)).toMatchObject({
      ...brandFields,
      settingsConfig: {
        name: "tokensmarket",
        base_url: openAiBaseUrl,
        api_key: "",
        api_mode: "chat_completions",
        models: [
          {
            id: "claude-sonnet-5",
            context_length: 1000000,
          },
          { id: "gpt-5.6-luna", context_length: 1050000 },
          { id: "gemini-3.5-flash", context_length: 1048576 },
        ],
      },
      suggestedDefaults: {
        model: { default: "claude-sonnet-5", provider: "tokensmarket" },
      },
    });
  });

  it("registers the Token Market brand icon", () => {
    expect(getIconUrl("tokensmarket")).toBeTruthy();
    expect(getIconMetadata("tokensmarket")).toMatchObject({
      displayName: "Token Market",
      defaultColor: "currentColor",
    });
  });
});
