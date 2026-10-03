import { describe, expect, it } from "vitest";

import { claudeDesktopProviderPresets } from "./claudeDesktopProviderPresets";
import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";
import { hermesProviderPresets } from "./hermesProviderPresets";
import { openclawProviderPresets } from "./openclawProviderPresets";
import { opencodeProviderPresets } from "./opencodeProviderPresets";
import { piProviderPresets } from "./piProviderPresets";
import { getIcon, getIconMetadata } from "../icons/extracted";

const collections = [
  ["Claude Code", providerPresets],
  ["Claude Desktop", claudeDesktopProviderPresets],
  ["Codex", codexProviderPresets],
  ["OpenCode", opencodeProviderPresets],
  ["OpenClaw", openclawProviderPresets],
  ["Hermes", hermesProviderPresets],
  ["Pi", piProviderPresets],
] as const;

const modelId = "Kimi-K3";
const modelName = "Kimi K3";
const anthropicEndpoint = "https://microquickjs.com";
const openAiEndpoint = "https://microquickjs.com/v1";
const brandFields = {
  websiteUrl: "https://ainetcafe.com/k3/?utm_source=cc-switch",
  apiKeyUrl: "https://microquickjs.com/register?aff=qjpC&lng=en",
  category: "aggregator",
  icon: "ainetcafe",
  iconColor: "#32FEA5",
};

function getPreset<T extends { name: string }>(presets: readonly T[]) {
  return presets.find((preset) => preset.name === "ainetcafe");
}

describe("ainetcafe provider presets", () => {
  it.each(collections)(
    "%s registers exactly one ainetcafe preset",
    (_n, presets) => {
      expect(
        presets.filter((preset) => preset.name === "ainetcafe"),
      ).toHaveLength(1);
    },
  );

  it("configures Claude Code with the Anthropic-compatible root", () => {
    expect(getPreset(providerPresets)).toMatchObject({
      ...brandFields,
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: anthropicEndpoint,
          ANTHROPIC_AUTH_TOKEN: "",
          ANTHROPIC_MODEL: modelId,
          ANTHROPIC_DEFAULT_HAIKU_MODEL: modelId,
          ANTHROPIC_DEFAULT_SONNET_MODEL: modelId,
          ANTHROPIC_DEFAULT_OPUS_MODEL: modelId,
        },
      },
      endpointCandidates: [anthropicEndpoint],
      modelsUrl: `${openAiEndpoint}/models`,
    });
  });

  it("configures Claude Desktop as an Anthropic proxy", () => {
    expect(getPreset(claudeDesktopProviderPresets)).toMatchObject({
      ...brandFields,
      baseUrl: anthropicEndpoint,
      mode: "proxy",
      apiFormat: "anthropic",
      modelRoutes: [
        {
          routeId: "claude-sonnet-5",
          upstreamModel: modelId,
          labelOverride: modelId,
        },
      ],
      endpointCandidates: [anthropicEndpoint],
    });
  });

  it("configures Codex for OpenAI Chat with reasoning_effort passthrough", () => {
    const codex = getPreset(codexProviderPresets)!;
    expect(codex).toMatchObject({
      ...brandFields,
      auth: { OPENAI_API_KEY: "" },
      endpointCandidates: [openAiEndpoint],
      apiFormat: "openai_chat",
      modelCatalog: [
        {
          model: modelId,
          displayName: modelName,
          contextWindow: 262144,
          inputModalities: ["text", "image"],
          reasoningLevels: ["low", "high", "max"],
          defaultReasoningLevel: "high",
        },
      ],
      codexChatReasoning: {
        supportsThinking: false,
        supportsEffort: true,
        thinkingParam: "none",
        effortParam: "reasoning_effort",
        effortValueMode: "passthrough",
        outputFormat: "reasoning_content",
      },
    });
    expect(codex.config).toContain(`model = "${modelId}"`);
    expect(codex.config).toContain(`base_url = "${openAiEndpoint}"`);
  });

  it("configures OpenCode, OpenClaw and Hermes on the versioned OpenAI base", () => {
    const opencode = getPreset(opencodeProviderPresets)!;
    expect(opencode.settingsConfig.options.baseURL).toBe(openAiEndpoint);
    expect(Object.keys(opencode.settingsConfig.models)).toEqual([modelId]);

    const openclaw = getPreset(openclawProviderPresets)!;
    expect(openclaw.settingsConfig.baseUrl).toBe(openAiEndpoint);
    expect(openclaw.settingsConfig.models?.[0]).toMatchObject({
      id: modelId,
      reasoning: true,
      input: ["text", "image"],
      cost: { input: 2.1, output: 10.5, cacheRead: 0.3 },
    });

    const hermes = getPreset(hermesProviderPresets)!;
    expect(hermes.settingsConfig.base_url).toBe(openAiEndpoint);
    expect(hermes.settingsConfig.models?.[0]).toMatchObject({
      id: modelId,
      name: modelName,
    });
  });

  it("configures Pi from the shared Kimi K3 catalog entry", () => {
    const pi = getPreset(piProviderPresets)!;
    expect(pi.settingsConfig.baseUrl).toBe(openAiEndpoint);
    expect(pi.settingsConfig.api).toBe("openai-completions");
    expect(pi.settingsConfig.models?.[0]).toMatchObject({
      id: modelId,
      reasoning: true,
    });
  });

  it("ships a themed icon", () => {
    expect(getIcon("ainetcafe")).toContain("<svg");
    expect(getIconMetadata("ainetcafe")).toMatchObject({
      name: "ainetcafe",
      displayName: "ainetcafe",
      category: "ai-provider",
      defaultColor: "#32FEA5",
    });
  });
});
