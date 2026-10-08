import { describe, expect, it } from "vitest";

import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";
import { hermesProviderPresets } from "./hermesProviderPresets";
import { openclawProviderPresets } from "./openclawProviderPresets";
import { opencodeProviderPresets } from "./opencodeProviderPresets";
import { getIcon, getIconMetadata } from "../icons/extracted";

const presetGroups = [
  ["Claude Code", providerPresets],
  ["Codex", codexProviderPresets],
  ["OpenCode", opencodeProviderPresets],
  ["OpenClaw", openclawProviderPresets],
  ["Hermes", hermesProviderPresets],
] as const;

const anthropicBaseUrl = "https://api.powertokens.ai";
const openAiBaseUrl = "https://api.powertokens.ai/v1";
const brandDetails = {
  websiteUrl: "https://powertokens.ai?utm_source=github&utm_medium=cc-switch&utm_campaign=provider-preset",
  apiKeyUrl: "https://www.powertokens.ai/en/api-keys?utm_source=github&utm_medium=cc-switch&utm_campaign=provider-preset",
  category: "aggregator",
  icon: "powertokens",
  iconColor: "#7B37C3",
};

function findPowerTokens<T extends { name: string }>(entries: readonly T[]) {
  return entries.find((entry) => entry.name === "PowerTokens");
}

describe("PowerTokens provider presets", () => {
  it.each(presetGroups)(
    "%s registers exactly one PowerTokens preset",
    (_surface, entries) => {
      expect(
        entries.filter((entry) => entry.name === "PowerTokens"),
      ).toHaveLength(1);
      expect(
        findPowerTokens(entries as readonly { name: string }[]),
      ).toMatchObject(brandDetails);
    },
  );

  it("configures Claude Code with the Anthropic Messages endpoint", () => {
    expect(findPowerTokens(providerPresets)).toMatchObject({
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: anthropicBaseUrl,
          ANTHROPIC_AUTH_TOKEN: "",
          ANTHROPIC_MODEL: "glm-5.2",
          ANTHROPIC_DEFAULT_HAIKU_MODEL: "glm-5-turbo",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.2",
          ANTHROPIC_DEFAULT_OPUS_MODEL: "MiniMax-M3",
        },
      },
      endpointCandidates: [anthropicBaseUrl],
      modelsUrl: `${openAiBaseUrl}/models`,
    });
  });

  it("routes Codex through Chat Completions", () => {
    const preset = findPowerTokens(codexProviderPresets)!;
    expect(preset.apiFormat).toBe("openai_chat");
    expect(preset.config).toContain(`base_url = "${openAiBaseUrl}"`);
    expect(preset.config).toContain('model = "glm-5.2"');
    expect(preset.endpointCandidates).toEqual([openAiBaseUrl]);
  });

  it("configures OpenCode, OpenClaw and Hermes with the OpenAI-compatible endpoint", () => {
    expect(findPowerTokens(opencodeProviderPresets)).toMatchObject({
      settingsConfig: {
        npm: "@ai-sdk/openai-compatible",
        options: { baseURL: openAiBaseUrl, apiKey: "" },
      },
    });
    expect(findPowerTokens(openclawProviderPresets)).toMatchObject({
      settingsConfig: {
        baseUrl: openAiBaseUrl,
        api: "openai-completions",
      },
      suggestedDefaults: { model: { primary: "powertokens/glm-5.2" } },
    });
    expect(findPowerTokens(hermesProviderPresets)).toMatchObject({
      settingsConfig: {
        base_url: openAiBaseUrl,
        api_mode: "chat_completions",
      },
      suggestedDefaults: {
        model: { default: "glm-5.2", provider: "powertokens" },
      },
    });
  });

  it("registers the PowerTokens icon", () => {
    expect(getIcon("powertokens")).toContain("<title>PowerTokens</title>");
    expect(getIconMetadata("powertokens")).toMatchObject({
      displayName: "PowerTokens",
      defaultColor: "#7B37C3",
    });
  });
});
