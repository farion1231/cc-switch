import { describe, expect, it } from "vitest";

import { claudeDesktopProviderPresets } from "./claudeDesktopProviderPresets";
import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";
import { hermesProviderPresets } from "./hermesProviderPresets";
import { openclawProviderPresets } from "./openclawProviderPresets";
import { opencodeProviderPresets } from "./opencodeProviderPresets";
import { piProviderPresets } from "./piProviderPresets";
import { getIcon, getIconMetadata } from "../icons/extracted";

const ppioPresetCollections = [
  ["Claude Code", providerPresets],
  ["Claude Desktop", claudeDesktopProviderPresets],
  ["Codex", codexProviderPresets],
  ["OpenCode", opencodeProviderPresets],
  ["OpenClaw", openclawProviderPresets],
  ["Hermes", hermesProviderPresets],
  ["Pi", piProviderPresets],
] as const;

const ppioModelId = "deepseek/deepseek-v4-flash-0731";
const ppioModelName = "Deepseek V4 Flash 0731";
const ppioAnthropicEndpoint = "https://api.ppio.com/anthropic";
const ppioOpenAiEndpoint = "https://api.ppio.com/openai/v1";
const ppioChatCompletionsEndpoint = `${ppioOpenAiEndpoint}/chat/completions`;
const ppioModelsEndpoint = `${ppioOpenAiEndpoint}/models`;
const ppioBrandFields = {
  websiteUrl: "https://ppio.com",
  apiKeyUrl: "https://ppio.com/activity/ccswitch",
  category: "aggregator",
  isPartner: true,
  partnerPromotionKey: "ppio",
  icon: "ppio",
  iconColor: "#2874FF",
};

function getPpioPreset<T extends { name: string }>(presets: readonly T[]) {
  return presets.find((preset) => preset.name === "PPIO");
}

describe("PPIO provider presets", () => {
  it.each(ppioPresetCollections)(
    "%s registers exactly one PPIO preset",
    (_name, presets) => {
      expect(presets.filter((preset) => preset.name === "PPIO")).toHaveLength(
        1,
      );
    },
  );

  it("configures Claude Code with the native Anthropic endpoint", () => {
    const claude = getPpioPreset(providerPresets)!;
    expect(claude).toMatchObject({
      ...ppioBrandFields,
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: ppioAnthropicEndpoint,
          ANTHROPIC_AUTH_TOKEN: "",
          ANTHROPIC_MODEL: ppioModelId,
          ANTHROPIC_DEFAULT_HAIKU_MODEL: ppioModelId,
          ANTHROPIC_DEFAULT_SONNET_MODEL: ppioModelId,
          ANTHROPIC_DEFAULT_OPUS_MODEL: ppioModelId,
        },
      },
      endpointCandidates: [ppioAnthropicEndpoint],
      modelsUrl: ppioModelsEndpoint,
    });
  });

  it("configures Claude Desktop with the native Anthropic endpoint", () => {
    expect(getPpioPreset(claudeDesktopProviderPresets)).toMatchObject({
      ...ppioBrandFields,
      baseUrl: ppioAnthropicEndpoint,
      mode: "proxy",
      apiFormat: "anthropic",
      modelRoutes: [
        {
          routeId: "claude-sonnet-5",
          upstreamModel: ppioModelId,
          labelOverride: ppioModelId,
          supports1m: true,
        },
      ],
      endpointCandidates: [ppioAnthropicEndpoint],
    });
  });

  it("configures Codex for OpenAI Chat translation and reasoning", () => {
    const codex = getPpioPreset(codexProviderPresets)!;
    expect(codex).toMatchObject({
      ...ppioBrandFields,
      auth: { OPENAI_API_KEY: "" },
      endpointCandidates: [ppioOpenAiEndpoint],
      apiFormat: "openai_chat",
      modelCatalog: [
        {
          model: ppioModelId,
          displayName: ppioModelName,
          contextWindow: 1048576,
          inputModalities: ["text"],
        },
      ],
      codexChatReasoning: {
        supportsThinking: true,
        supportsEffort: false,
        thinkingParam: "thinking",
        effortParam: "none",
        outputFormat: "reasoning_content",
      },
    });
    expect(codex.config).toContain(`model = "${ppioModelId}"`);
    expect(codex.config).toContain(`base_url = "${ppioOpenAiEndpoint}"`);
    expect(codex.config).toContain('wire_api = "responses"');
  });

  it("configures OpenCode with a versioned OpenAI-compatible base", () => {
    const opencode = getPpioPreset(opencodeProviderPresets)!;
    expect(opencode).toMatchObject({
      ...ppioBrandFields,
      settingsConfig: {
        npm: "@ai-sdk/openai-compatible",
        name: "PPIO",
        options: {
          baseURL: ppioOpenAiEndpoint,
          apiKey: "",
          setCacheKey: true,
        },
        models: { [ppioModelId]: { name: ppioModelName } },
      },
      templateValues: {
        apiKey: { label: "API Key", placeholder: "", editorValue: "" },
      },
    });
    expect(Object.keys(opencode.settingsConfig.models)).toEqual([ppioModelId]);
    expect(`${opencode.settingsConfig.options.baseURL}/chat/completions`).toBe(
      ppioChatCompletionsEndpoint,
    );
  });

  it("configures OpenClaw with a versioned OpenAI Chat base", () => {
    const openclaw = getPpioPreset(openclawProviderPresets)!;
    expect(openclaw).toMatchObject({
      ...ppioBrandFields,
      settingsConfig: {
        baseUrl: ppioOpenAiEndpoint,
        apiKey: "",
        api: "openai-completions",
        models: [
          {
            id: ppioModelId,
            name: ppioModelName,
            reasoning: true,
            input: ["text"],
            contextWindow: 1048576,
            maxTokens: 393216,
            cost: { input: 0.14, output: 0.29, cacheRead: 0.03 },
          },
        ],
      },
      templateValues: {
        apiKey: { label: "API Key", placeholder: "sk-...", editorValue: "" },
      },
      suggestedDefaults: {
        model: { primary: `ppio/${ppioModelId}` },
        modelCatalog: { [`ppio/${ppioModelId}`]: { alias: ppioModelName } },
      },
    });
    expect(`${openclaw.settingsConfig.baseUrl}/chat/completions`).toBe(
      ppioChatCompletionsEndpoint,
    );
  });

  it("configures Hermes with a versioned OpenAI Chat base", () => {
    const hermes = getPpioPreset(hermesProviderPresets)!;
    expect(hermes).toMatchObject({
      ...ppioBrandFields,
      settingsConfig: {
        name: "ppio",
        base_url: ppioOpenAiEndpoint,
        api_key: "",
        api_mode: "chat_completions",
        models: [
          {
            id: ppioModelId,
            name: ppioModelName,
            context_length: 1048576,
          },
        ],
      },
      suggestedDefaults: {
        model: { default: ppioModelId, provider: "ppio" },
      },
    });
    expect(`${hermes.settingsConfig.base_url}/chat/completions`).toBe(
      ppioChatCompletionsEndpoint,
    );
  });

  it("configures Pi with a versioned OpenAI Chat base", () => {
    const pi = getPpioPreset(piProviderPresets)!;
    expect(pi).toMatchObject({
      ...ppioBrandFields,
      providerKey: "cc-switch-ppio",
      settingsConfig: {
        name: "PPIO",
        baseUrl: ppioOpenAiEndpoint,
        api: "openai-completions",
        apiKey: "",
        models: [
          {
            id: ppioModelId,
            name: ppioModelName,
            reasoning: true,
            input: ["text"],
            contextWindow: 1048576,
            maxTokens: 393216,
            thinkingLevelMap: {
              minimal: null,
              low: null,
              medium: null,
              high: "high",
              max: "max",
            },
            compat: {
              supportsStore: false,
              supportsDeveloperRole: false,
              maxTokensField: "max_tokens",
              requiresReasoningContentOnAssistantMessages: true,
              thinkingFormat: "deepseek",
            },
          },
        ],
      },
    });
    expect(`${pi.settingsConfig.baseUrl}/chat/completions`).toBe(
      ppioChatCompletionsEndpoint,
    );
  });

  it("registers the PPIO brand icon", () => {
    expect(getIcon("ppio")).toContain("<title>PPIO</title>");
    expect(getIconMetadata("ppio")).toMatchObject({
      displayName: "PPIO",
      defaultColor: "#2874FF",
    });
  });
});
