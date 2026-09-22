import { describe, expect, it } from "vitest";

import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { mcodeProviderPresets } from "@/config/mcodeProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { piProviderPresets } from "@/config/piProviderPresets";
import { getIcon, hasIcon } from "@/icons/extracted";

const WEBSITE_URL = "https://moark.com";
const API_KEY_URL = "https://moark.com/dashboard/tokens";
const DEFAULT_MODEL = "deepseek-v4-flash-0731";

describe("MoArk (模力方舟) provider presets", () => {
  it("uses the Anthropic-native root endpoint for Claude Code", () => {
    const preset = providerPresets.find((item) => item.name === "模力方舟");

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.endpointCandidates).toEqual(["https://moark.com/anthropic"]);
    expect(preset?.icon).toBe("moark");

    const env = (preset?.settingsConfig as { env: Record<string, string> }).env;
    expect(env.ANTHROPIC_BASE_URL).toBe("https://moark.com/anthropic");
    expect(env.ANTHROPIC_AUTH_TOKEN).toBe("");
    expect(env.ANTHROPIC_MODEL).toBe(DEFAULT_MODEL);
    expect(env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe(DEFAULT_MODEL);
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(DEFAULT_MODEL);
    expect(env.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe(DEFAULT_MODEL);
  });

  it("uses the OpenAI-compatible Responses endpoint for Codex", () => {
    const preset = codexProviderPresets.find(
      (item) => item.name === "模力方舟",
    );

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.endpointCandidates).toEqual(["https://moark.com/v1"]);
    expect(preset?.icon).toBe("moark");
    expect(preset?.auth).toEqual({ OPENAI_API_KEY: "" });
    expect(preset?.config).toContain('model = "deepseek-v4-flash-0731"');
    expect(preset?.config).toContain('base_url = "https://moark.com/v1"');
    expect(preset?.config).toContain('wire_api = "responses"');
  });

  it("uses the OpenAI-compatible endpoint for Pi", () => {
    const preset = piProviderPresets.find((item) => item.name === "模力方舟");

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.icon).toBe("moark");
    expect(preset?.providerKey).toBe("cc-switch-moark");
    expect(preset?.settingsConfig.baseUrl).toBe("https://api.moark.com/v1");
    expect(preset?.settingsConfig.api).toBe("openai-completions");
    expect(preset?.settingsConfig.apiKey).toBe("");

    const modelIds = preset?.settingsConfig.models.map((model) => model.id);
    expect(modelIds).toEqual(
      expect.arrayContaining([
        DEFAULT_MODEL,
        "DeepSeek-V4-Pro",
        "GLM-5.3",
        "Kimi-K2.7-Code",
        "qwen3-coder-plus",
      ]),
    );
  });

  it("uses the OpenAI-compatible endpoint for OpenCode", () => {
    const preset = opencodeProviderPresets.find(
      (item) => item.name === "模力方舟",
    );
    const models = preset?.settingsConfig.models ?? {};

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.icon).toBe("moark");
    expect(preset?.settingsConfig.npm).toBe("@ai-sdk/openai-compatible");
    expect(preset?.settingsConfig.options?.baseURL).toBe(
      "https://api.moark.com/v1",
    );
    expect(models).toHaveProperty(DEFAULT_MODEL);
    expect(models).toHaveProperty("Kimi-K2.7-Code");
    expect(models[DEFAULT_MODEL]?.name).toBe("DeepSeek V4 Flash");
  });

  it("uses the OpenAI Completions endpoint for OpenClaw", () => {
    const preset = openclawProviderPresets.find(
      (item) => item.name === "模力方舟",
    );
    const modelIds = (preset?.settingsConfig.models ?? []).map(
      (model) => model.id,
    );

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.icon).toBe("moark");
    expect(preset?.settingsConfig.baseUrl).toBe("https://api.moark.com/v1");
    expect(preset?.settingsConfig.api).toBe("openai-completions");
    expect(modelIds).toEqual(
      expect.arrayContaining([
        DEFAULT_MODEL,
        "DeepSeek-V4-Pro",
        "GLM-5.3",
        "Kimi-K2.7-Code",
      ]),
    );
    expect(preset?.suggestedDefaults?.model).toEqual({
      primary: `moark/${DEFAULT_MODEL}`,
    });
    expect(preset?.suggestedDefaults?.modelCatalog).toHaveProperty(
      `moark/${DEFAULT_MODEL}`,
    );
  });

  it("inherits the Pi preset for MiniMax Code", () => {
    const preset = mcodeProviderPresets.find((item) => item.name === "模力方舟");

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig.kind).toBe("custom");
    expect(preset?.settingsConfig.api).toBe("openai-completions");
    expect(preset?.settingsConfig.options.baseURL).toBe(
      "https://api.moark.com/v1",
    );
    expect(Object.keys(preset?.settingsConfig.models ?? {})).toContain(
      DEFAULT_MODEL,
    );
  });

  it("registers the MoArk mark in the icon registry", () => {
    expect(hasIcon("moark")).toBe(true);
    expect(getIcon("moark")).toContain("<title>MoArk</title>");
    expect(getIcon("moark")).toContain('viewBox="0 0 88 88"');
  });
});
