import { describe, expect, it } from "vitest";

import {
  getDeepSeekHarnessPresetEntries,
  deepseekHarnessProviderPresets,
} from "@/config/deepseekHarnessProviderPresets";

describe("DeepSeek Harness provider presets", () => {
  it("provides the official provider defaults", () => {
    expect(deepseekHarnessProviderPresets).toHaveLength(1);

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

  it("maps presets for the provider form", () => {
    expect(getDeepSeekHarnessPresetEntries()).toEqual([
      {
        id: "deepseek-harness-0",
        preset: deepseekHarnessProviderPresets[0],
      },
    ]);
  });
});
