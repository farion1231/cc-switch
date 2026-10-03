import { describe, expect, it } from "vitest";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";

describe("Modelsell OpenCode provider preset", () => {
  const preset = opencodeProviderPresets.find(
    (provider) => provider.name === "Modelsell",
  );

  it("registers Modelsell as an OpenAI-compatible aggregator", () => {
    expect(preset).toBeDefined();
    expect(preset).toMatchObject({
      websiteUrl: "https://modelsell.com",
      apiKeyUrl: "https://modelsell.com/console/token",
      category: "aggregator",
      settingsConfig: {
        npm: "@ai-sdk/openai-compatible",
        name: "Modelsell",
        options: {
          baseURL: "https://modelsell.com/v1",
          apiKey: "",
          setCacheKey: true,
        },
      },
    });
  });

  it("keeps model selection dynamic", () => {
    expect(preset!.settingsConfig.models).toEqual({});
  });

  it("uses the Modelsell base URL in the editable template", () => {
    expect(preset!.templateValues?.baseURL).toMatchObject({
      placeholder: "https://modelsell.com/v1",
      defaultValue: "https://modelsell.com/v1",
    });
  });
});
