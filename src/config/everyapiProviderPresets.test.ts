import { describe, expect, it } from "vitest";

import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";

const gatewayOrigin = "https://api.everyapi.ai";
const cnGatewayOrigin = "https://api-cn.everyapi.ai";
const openAiBaseUrl = `${gatewayOrigin}/v1`;
const cnOpenAiBaseUrl = `${cnGatewayOrigin}/v1`;
const brandDetails = {
  websiteUrl: "https://everyapi.ai",
  apiKeyUrl: "https://app.everyapi.ai/keys",
  category: "aggregator",
};

const allEveryApiPresetGroups = [
  ["Claude Code", providerPresets],
  ["Codex", codexProviderPresets],
] as const;

function findEveryApiEntry<T extends { name: string }>(entries: readonly T[]) {
  return entries.find((entry) => entry.name === "EveryAPI");
}

describe("EveryAPI provider presets", () => {
  it.each(allEveryApiPresetGroups)(
    "%s registers exactly one EveryAPI preset",
    (_surface, entries) => {
      expect(entries.filter((entry) => entry.name === "EveryAPI")).toHaveLength(
        1,
      );
    },
  );

  it("configures Claude Code with the bare gateway origin", () => {
    const preset = findEveryApiEntry(providerPresets)!;
    expect(preset).toMatchObject({
      ...brandDetails,
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: gatewayOrigin,
          ANTHROPIC_AUTH_TOKEN: "",
        },
      },
      endpointCandidates: [gatewayOrigin, cnGatewayOrigin],
    });
  });

  it("keeps the Claude base URL free of a version suffix", () => {
    // The Anthropic client appends its own /v1; baking one into the base URL
    // produces /v1/v1/messages, which the gateway rejects.
    const preset = findEveryApiEntry(providerPresets)!;
    const env = (preset.settingsConfig as { env: Record<string, string> }).env;
    expect(env.ANTHROPIC_BASE_URL).not.toMatch(/\/v1\/?$/);
    for (const candidate of preset.endpointCandidates ?? []) {
      expect(candidate).not.toMatch(/\/v1\/?$/);
    }
  });

  it("forwards Claude model names instead of pinning one model", () => {
    // The gateway routes whatever Claude model the client asks for, so the
    // preset must not override the model the user (or Claude Code) selected.
    const preset = findEveryApiEntry(providerPresets)!;
    const env = (preset.settingsConfig as { env: Record<string, string> }).env;
    expect(Object.keys(env).sort()).toEqual([
      "ANTHROPIC_AUTH_TOKEN",
      "ANTHROPIC_BASE_URL",
    ]);
  });

  it("configures Codex against the OpenAI-compatible base", () => {
    const preset = findEveryApiEntry(codexProviderPresets)!;
    expect(preset).toMatchObject({
      ...brandDetails,
      auth: { OPENAI_API_KEY: "" },
      endpointCandidates: [openAiBaseUrl, cnOpenAiBaseUrl],
    });
    expect(preset.config).toContain(`base_url = "${openAiBaseUrl}"`);
    expect(preset.config).toContain('name = "EveryAPI"');
    expect(preset.config).toContain('wire_api = "responses"');
  });

  it("points both surfaces at the same gateway host", () => {
    const claudePreset = findEveryApiEntry(providerPresets)!;
    const codexPreset = findEveryApiEntry(codexProviderPresets)!;
    const claudeEnv = (
      claudePreset.settingsConfig as { env: Record<string, string> }
    ).env;
    expect(codexPreset.config).toContain(
      `base_url = "${claudeEnv.ANTHROPIC_BASE_URL}/v1"`,
    );
  });
});
