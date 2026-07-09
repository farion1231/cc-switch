import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { hasIcon } from "@/icons/extracted";

const WEBSITE_URL = "https://unorouter.com";
const API_KEY_URL = "https://unorouter.com/token";

describe("UnoRouter provider presets", () => {
  it("uses the Anthropic-compatible root endpoint for Claude", () => {
    const preset = providerPresets.find((item) => item.name === "UnoRouter");

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.apiKeyField).toBe("ANTHROPIC_API_KEY");
    expect(preset?.icon).toBe("unorouter");

    const env = (preset?.settingsConfig as { env: Record<string, string> }).env;
    expect(env.ANTHROPIC_BASE_URL).toBe("https://api.unorouter.com");
    expect(env.ANTHROPIC_API_KEY).toBe("");
  });

  it("uses the OpenAI-compatible v1 endpoint for Codex", () => {
    const preset = codexProviderPresets.find(
      (item) => item.name === "UnoRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.endpointCandidates).toEqual([
      "https://api.unorouter.com/v1",
    ]);
    expect(preset?.auth).toEqual({ OPENAI_API_KEY: "" });
    expect(preset?.config).toContain('name = "unorouter"');
    expect(preset?.config).toContain('model = "gpt-5.5"');
    expect(preset?.config).toContain(
      'base_url = "https://api.unorouter.com/v1"',
    );
    expect(preset?.config).toContain('wire_api = "responses"');
  });

  it("uses the Gemini-compatible v1beta endpoint for Gemini", () => {
    const preset = geminiProviderPresets.find(
      (item) => item.name === "UnoRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.baseURL).toBe("https://api.unorouter.com/v1beta");
    expect(preset?.endpointCandidates).toEqual([
      "https://api.unorouter.com/v1beta",
    ]);
    expect(preset?.model).toBe("gemini-3.5-flash");

    const env = (preset?.settingsConfig as { env: Record<string, string> }).env;
    expect(env.GOOGLE_GEMINI_BASE_URL).toBe("https://api.unorouter.com/v1beta");
    expect(env.GEMINI_MODEL).toBe("gemini-3.5-flash");
  });

  it("uses OpenAI-compatible config for OpenCode", () => {
    const preset = opencodeProviderPresets.find(
      (item) => item.name === "UnoRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig.npm).toBe("@ai-sdk/openai-compatible");
    expect(preset?.settingsConfig.options?.baseURL).toBe(
      "https://api.unorouter.com/v1",
    );
    expect(preset?.settingsConfig.options?.apiKey).toBe("");
    expect(preset?.settingsConfig.models).toHaveProperty("gpt-5.5");
  });

  it("uses OpenAI completions config for OpenClaw without hardcoded pricing", () => {
    const preset = openclawProviderPresets.find(
      (item) => item.name === "UnoRouter",
    );
    const [model] = preset?.settingsConfig.models ?? [];

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig.baseUrl).toBe("https://api.unorouter.com/v1");
    expect(preset?.settingsConfig.api).toBe("openai-completions");
    expect(model).toMatchObject({
      id: "gpt-5.5",
      name: "GPT-5.5",
      contextWindow: 400000,
    });
    expect(model).not.toHaveProperty("cost");
    expect(preset?.suggestedDefaults?.model).toEqual({
      primary: "unorouter/gpt-5.5",
    });
  });

  it("uses chat completions config for Hermes", () => {
    const preset = hermesProviderPresets.find(
      (item) => item.name === "UnoRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig).toMatchObject({
      name: "unorouter",
      base_url: "https://api.unorouter.com/v1",
      api_key: "",
      api_mode: "chat_completions",
    });
    expect(preset?.suggestedDefaults?.model).toEqual({
      default: "gpt-5.5",
      provider: "unorouter",
    });
  });

  it("uses direct Anthropic routing for Claude Desktop", () => {
    const preset = claudeDesktopProviderPresets.find(
      (item) => item.name === "UnoRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.baseUrl).toBe("https://api.unorouter.com");
    expect(preset?.mode).toBe("direct");
    expect(preset?.apiFormat).toBe("anthropic");
    expect(preset?.modelRoutes?.length).toBeGreaterThan(0);
  });

  it("registers the UnoRouter provider icon", () => {
    expect(hasIcon("unorouter")).toBe(true);
  });
});
