import { describe, expect, it } from "vitest";
import { normalizeCursorProviders } from "@/lib/api/cursor";

describe("normalizeCursorProviders", () => {
  it("为旧 Cursor 配置补齐新增字段和默认值", () => {
    const providers = normalizeCursorProviders({
      legacy: {
        id: "legacy",
        name: "Legacy Model",
        settingsConfig: {
          enabled: true,
          type: "openai",
          baseURL: "https://api.example.com",
          apiKey: "secret",
          modelID: "legacy-model",
        },
      },
    });

    expect(providers.legacy.settingsConfig.providerGroup).toBe("");
    expect(providers.legacy.settingsConfig.contextWindowTokens).toBe(0);
    expect(providers.legacy.settingsConfig.openAIEndpoint).toBe(
      "/v1/responses",
    );
  });

  it("保留已有提供商分类", () => {
    const providers = normalizeCursorProviders({
      current: {
        id: "current",
        name: "Current Model",
        settingsConfig: {
          providerGroup: "OpenRouter",
          type: "openai",
        },
      },
    });

    expect(providers.current.settingsConfig.providerGroup).toBe("OpenRouter");
  });
});
