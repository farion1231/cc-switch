import { describe, expect, it } from "vitest";

import {
  getDeepSeekHarnessPresetEntries,
  deepseekHarnessProviderPresets,
} from "@/config/deepseekHarnessProviderPresets";

describe("DeepSeek Harness provider presets", () => {
  it("provides the official provider defaults", () => {
    expect(deepseekHarnessProviderPresets).toHaveLength(2);

    const preset = deepseekHarnessProviderPresets[0];
    expect(preset).toMatchObject({
      id: "deepseek-official",
      name: "DeepSeek",
      category: "official",
      icon: "deepseek",
      websiteUrl: "https://platform.deepseek.com",
      apiKeyUrl: "https://platform.deepseek.com/api_keys",
      settingsConfig: {
        baseURL: "https://api.deepseek.com",
        models: [
          { id: "deepseek-v4-flash", name: "DeepSeek-V4-Flash" },
          { id: "deepseek-v4-pro", name: "DeepSeek-V4-Pro" },
        ],
      },
    });
  });

  it("provides an OpenAI-compatible preset", () => {
    expect(deepseekHarnessProviderPresets[1]).toMatchObject({
      id: "dsh-openai-compatible",
      name: "OpenAI Compatible",
      category: "custom",
      icon: "openai",
      websiteUrl: "https://api.openai.com",
      settingsConfig: {
        apiKey: "",
        baseURL: "https://api.openai.com/v1",
        displayName: "OpenAI Compatible",
        api: "openai-completions",
        apiKeyEnv: "OPENAI_API_KEY",
        models: [],
      },
    });
  });

  it("maps presets for the provider form", () => {
    expect(getDeepSeekHarnessPresetEntries()).toEqual([
      {
        id: "deepseek-harness-0",
        preset: deepseekHarnessProviderPresets[0],
      },
      {
        id: "deepseek-harness-1",
        preset: deepseekHarnessProviderPresets[1],
      },
    ]);
  });
});
