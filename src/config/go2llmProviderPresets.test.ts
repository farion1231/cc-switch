import { describe, expect, it } from "vitest";

import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";

describe("go2llm provider presets", () => {
  it("configures Claude Code for the Anthropic-compatible endpoint", () => {
    const preset = providerPresets.find((item) => item.name === "go2llm");

    expect(preset).toMatchObject({
      websiteUrl: "https://go2llm.tech",
      apiKeyUrl: "https://go2llm.tech/app",
      category: "aggregator",
      endpointCandidates: ["https://go2llm.tech"],
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://go2llm.tech",
          ANTHROPIC_AUTH_TOKEN: "",
        },
      },
    });
    expect(preset).not.toHaveProperty("isPartner");
    expect(preset).not.toHaveProperty("primePartner");
    expect(preset).not.toHaveProperty("partnerPromotionKey");
  });

  it("configures Codex for the OpenAI-compatible endpoint", () => {
    const preset = codexProviderPresets.find((item) => item.name === "go2llm");

    expect(preset).toMatchObject({
      websiteUrl: "https://go2llm.tech",
      apiKeyUrl: "https://go2llm.tech/app",
      auth: { OPENAI_API_KEY: "" },
      endpointCandidates: ["https://go2llm.tech/v1"],
      category: "aggregator",
    });
    expect(preset?.config).toContain('base_url = "https://go2llm.tech/v1"');
    expect(preset?.config).toContain('wire_api = "responses"');
    expect(preset).not.toHaveProperty("isPartner");
    expect(preset).not.toHaveProperty("primePartner");
    expect(preset).not.toHaveProperty("partnerPromotionKey");
  });
});
