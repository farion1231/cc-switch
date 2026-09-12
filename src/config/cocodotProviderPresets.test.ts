import { describe, expect, it } from "vitest";

import { getIcon, getIconMetadata } from "../icons/extracted";
import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";

describe("cocodot provider presets", () => {
  it("configures Claude Code for the native Anthropic endpoint", () => {
    const preset = providerPresets.find((item) => item.name === "cocodot");

    expect(preset).toMatchObject({
      websiteUrl: "https://cocodot.co/ccswitch",
      apiKeyUrl: "https://cocodot.co/dashboard/ai",
      category: "aggregator",
      endpointCandidates: ["https://cocodot.co/api/ai"],
      modelsUrl: "https://cocodot.co/api/ai/v1/models",
      icon: "cocodot",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://cocodot.co/api/ai",
          ANTHROPIC_AUTH_TOKEN: "",
          ANTHROPIC_MODEL: "mcs-6",
          ANTHROPIC_DEFAULT_HAIKU_MODEL: "mch-1",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "mcs-6",
          ANTHROPIC_DEFAULT_OPUS_MODEL: "mco-6",
        },
      },
    });
    expect(preset?.apiFormat).toBeUndefined();
  });

  it("configures Codex for OpenAI Chat Completions routing", () => {
    const preset = codexProviderPresets.find((item) => item.name === "cocodot");

    expect(preset).toMatchObject({
      websiteUrl: "https://cocodot.co/ccswitch",
      apiKeyUrl: "https://cocodot.co/dashboard/ai",
      auth: { OPENAI_API_KEY: "" },
      category: "aggregator",
      endpointCandidates: ["https://cocodot.co/api/ai/v1"],
      apiFormat: "openai_chat",
      icon: "cocodot",
    });
    expect(preset?.config).toContain('model = "mcs-6"');
    expect(preset?.config).toContain(
      'base_url = "https://cocodot.co/api/ai/v1"',
    );
    expect(preset?.config).toContain('wire_api = "responses"');
    expect(preset?.modelCatalog?.map((item) => item.model)).toEqual([
      "mcs-6",
      "mco-6",
      "mog-8-s",
      "mog-8-t",
      "mog-8-l",
    ]);
    expect(
      Object.fromEntries(
        (preset?.modelCatalog ?? []).map((item) => [
          item.model,
          item.contextWindow,
        ]),
      ),
    ).toEqual({
      "mcs-6": 1_000_000,
      "mco-6": 1_000_000,
      "mog-8-s": 272_000,
      "mog-8-t": 272_000,
      "mog-8-l": 272_000,
    });
  });

  it("ships a searchable cocodot icon", () => {
    expect(getIcon("cocodot")).toContain("<svg");
    expect(getIconMetadata("cocodot")).toMatchObject({
      displayName: "cocodot",
      defaultColor: "#2F6BFF",
    });
  });
});
