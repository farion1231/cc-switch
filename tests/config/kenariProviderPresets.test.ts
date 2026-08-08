import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { hasIcon } from "@/icons/extracted";

const WEBSITE_URL = "https://kenari.id";
const API_KEY_URL = "https://kenari.id/keys";

describe("Kenari provider presets", () => {
  it("uses the Anthropic-compatible root endpoint for Claude", () => {
    const preset = providerPresets.find((item) => item.name === "Kenari");

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.icon).toBe("kenari");

    const env = (preset?.settingsConfig as { env: Record<string, string> }).env;
    expect(env.ANTHROPIC_BASE_URL).toBe("https://kenari.id");
    expect(env.ANTHROPIC_AUTH_TOKEN).toBe("");
    expect(env.ANTHROPIC_MODEL).toBe("kimi-k2-7-code");
    expect(env.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe("glm-5-2");
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("kimi-k2-7-code");
    expect(env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe("deepseek-v4-flash");
  });

  it("uses the OpenAI-compatible v1 endpoint for Codex", () => {
    const preset = codexProviderPresets.find((item) => item.name === "Kenari");

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.endpointCandidates).toEqual(["https://kenari.id/v1"]);
    expect(preset?.auth).toEqual({ OPENAI_API_KEY: "" });
    expect(preset?.config).toContain('name = "kenari"');
    expect(preset?.config).toContain('model = "gpt-5-5"');
    expect(preset?.config).toContain('base_url = "https://kenari.id/v1"');
    expect(preset?.config).toContain('wire_api = "responses"');
  });

  it("uses OpenAI-compatible config for OpenCode", () => {
    const preset = opencodeProviderPresets.find(
      (item) => item.name === "Kenari",
    );

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig.npm).toBe("@ai-sdk/openai-compatible");
    expect(preset?.settingsConfig.options?.baseURL).toBe(
      "https://kenari.id/v1",
    );
    expect(preset?.settingsConfig.options?.apiKey).toBe("");
    expect(preset?.settingsConfig.models).toHaveProperty("glm-5-2");
  });

  it("uses OpenAI completions config for OpenClaw without hardcoded pricing", () => {
    const preset = openclawProviderPresets.find(
      (item) => item.name === "Kenari",
    );
    const [model] = preset?.settingsConfig.models ?? [];

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig.baseUrl).toBe("https://kenari.id/v1");
    expect(preset?.settingsConfig.api).toBe("openai-completions");
    expect(model).toMatchObject({
      id: "glm-5-2",
      name: "GLM 5.2",
      contextWindow: 1048576,
    });
    expect(model).not.toHaveProperty("cost");
    expect(preset?.suggestedDefaults?.model).toEqual({
      primary: "kenari/glm-5-2",
    });
  });

  it("uses chat completions config for Hermes", () => {
    const preset = hermesProviderPresets.find((item) => item.name === "Kenari");

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig).toMatchObject({
      name: "kenari",
      base_url: "https://kenari.id/v1",
      api_key: "",
      api_mode: "chat_completions",
    });
    expect(preset?.suggestedDefaults?.model).toEqual({
      default: "glm-5-2",
      provider: "kenari",
    });
  });

  it("uses direct Anthropic routing for Claude Desktop", () => {
    const preset = claudeDesktopProviderPresets.find(
      (item) => item.name === "Kenari",
    );

    expect(preset).toBeDefined();
    expect(preset?.baseUrl).toBe("https://kenari.id");
    expect(preset?.mode).toBe("direct");
    expect(preset?.apiFormat).toBe("anthropic");
    expect(preset?.modelRoutes?.length).toBeGreaterThan(0);
  });

  it("registers the Kenari provider icon", () => {
    expect(hasIcon("kenari")).toBe(true);
  });
});
