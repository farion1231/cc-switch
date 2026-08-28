import { describe, expect, it } from "vitest";

import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";
import { openclawProviderPresets } from "./openclawProviderPresets";

const baseUrl = "https://api.pzero.studio/v1";
const websiteUrl = "https://pzero.studio/agents";

describe("PZERO provider presets", () => {
  it("registers PZERO preset in Claude Code", () => {
    const preset = providerPresets.find((p) => p.name === "PZERO");
    expect(preset).toBeDefined();
    expect(preset).toMatchObject({
      name: "PZERO",
      websiteUrl,
      apiKeyUrl: websiteUrl,
      category: "aggregator",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: baseUrl,
          ANTHROPIC_AUTH_TOKEN: "",
          ANTHROPIC_MODEL: "claude-sonnet-5",
        },
      },
      endpointCandidates: [baseUrl],
    });
  });

  it("registers PZERO preset in Codex", () => {
    const preset = codexProviderPresets.find((p) => p.name === "PZERO");
    expect(preset).toBeDefined();
    expect(preset).toMatchObject({
      name: "PZERO",
      websiteUrl,
      apiKeyUrl: websiteUrl,
      category: "aggregator",
      auth: { OPENAI_API_KEY: "" },
      endpointCandidates: [baseUrl],
      apiFormat: "openai_chat",
    });
    expect(preset?.config).toContain(`base_url = "${baseUrl}"`);
  });

  it("registers PZERO preset in OpenClaw", () => {
    const preset = openclawProviderPresets.find((p) => p.name === "PZERO");
    expect(preset).toBeDefined();
    expect(preset).toMatchObject({
      name: "PZERO",
      websiteUrl,
      apiKeyUrl: websiteUrl,
      category: "aggregator",
      settingsConfig: {
        baseUrl,
        apiKey: "",
        api: "openai-completions",
      },
    });
  });
});
