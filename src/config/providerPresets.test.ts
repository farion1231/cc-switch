import { describe, expect, it } from "vitest";
import { providerPresets as claudeProviderPresets } from "./claudeProviderPresets";
import { openclawProviderPresets } from "./openclawProviderPresets";
import { opencodeProviderPresets } from "./opencodeProviderPresets";
import {
  BAIDU_QIANFAN_CODING_PLAN,
  BAIDU_QIANFAN_CODING_PLAN_MODELS,
} from "./baiduQianfanCodingPlan";

describe("Baidu Qianfan Coding Plan presets", () => {
  it("adds a Claude Code Anthropic-compatible preset", () => {
    const preset = claudeProviderPresets.find(
      (item) => item.name === BAIDU_QIANFAN_CODING_PLAN.name,
    );

    expect(preset).toBeDefined();
    expect(preset?.category).toBe("cn_official");
    expect(preset?.icon).toBe("baidu");
    expect(preset?.settingsConfig).toEqual({
      env: {
        ANTHROPIC_BASE_URL: BAIDU_QIANFAN_CODING_PLAN.anthropicBaseUrl,
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: BAIDU_QIANFAN_CODING_PLAN.defaultModel,
        ANTHROPIC_DEFAULT_HAIKU_MODEL: BAIDU_QIANFAN_CODING_PLAN.defaultModel,
        ANTHROPIC_DEFAULT_SONNET_MODEL: BAIDU_QIANFAN_CODING_PLAN.defaultModel,
        ANTHROPIC_DEFAULT_OPUS_MODEL: BAIDU_QIANFAN_CODING_PLAN.defaultModel,
      },
    });
  });

  it("adds an OpenCode OpenAI-compatible preset", () => {
    const preset = opencodeProviderPresets.find(
      (item) => item.name === BAIDU_QIANFAN_CODING_PLAN.name,
    );

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig).toEqual({
      npm: "@ai-sdk/openai-compatible",
      name: BAIDU_QIANFAN_CODING_PLAN.name,
      options: {
        baseURL: BAIDU_QIANFAN_CODING_PLAN.openaiBaseUrl,
        apiKey: "",
        setCacheKey: true,
      },
      models: BAIDU_QIANFAN_CODING_PLAN_MODELS,
    });
  });

  it("adds an OpenClaw OpenAI-compatible preset", () => {
    const preset = openclawProviderPresets.find(
      (item) => item.name === BAIDU_QIANFAN_CODING_PLAN.name,
    );

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig).toMatchObject({
      baseUrl: BAIDU_QIANFAN_CODING_PLAN.openaiBaseUrl,
      apiKey: "",
      api: "openai-completions",
    });
    expect(preset?.settingsConfig.models).toContainEqual(
      expect.objectContaining({
        id: BAIDU_QIANFAN_CODING_PLAN.defaultModel,
        name: "Qianfan Code Latest",
      }),
    );
    expect(preset?.suggestedDefaults).toEqual({
      model: {
        primary: "baidu/qianfan-code-latest",
        fallbacks: ["baidu/ernie-4.5-turbo-20260402"],
      },
      modelCatalog: {
        "baidu/qianfan-code-latest": { alias: "Qianfan" },
        "baidu/ernie-4.5-turbo-20260402": { alias: "ERNIE" },
      },
    });
  });
});
