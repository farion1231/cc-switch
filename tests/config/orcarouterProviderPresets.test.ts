import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { hasIcon } from "@/icons/extracted";

const WEBSITE_URL = "https://www.orcarouter.ai";
const API_KEY_URL = "https://www.orcarouter.ai/console";

describe("OrcaRouter provider presets", () => {
  it("uses the Anthropic-compatible root endpoint for Claude", () => {
    const preset = providerPresets.find((item) => item.name === "OrcaRouter");

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.icon).toBe("orcarouter");

    const env = (preset?.settingsConfig as { env: Record<string, string> }).env;
    expect(env.ANTHROPIC_BASE_URL).toBe("https://api.orcarouter.ai");
    expect(env.ANTHROPIC_AUTH_TOKEN).toBe("");
    // The gateway rejects bare Claude model ids, so every role must be pinned
    // to a namespaced model.
    expect(env.ANTHROPIC_MODEL).toBe("anthropic/claude-sonnet-5");
    expect(env.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe("anthropic/claude-opus-5");
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(
      "anthropic/claude-sonnet-5",
    );
    expect(env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe(
      "anthropic/claude-haiku-4.5",
    );
  });

  it("maps Claude Desktop routes to namespaced upstream models", () => {
    const preset = claudeDesktopProviderPresets.find(
      (item) => item.name === "OrcaRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.baseUrl).toBe("https://api.orcarouter.ai");
    expect(preset?.mode).toBe("proxy");
    expect(preset?.apiFormat).toBe("anthropic");
    expect(preset?.modelRoutes?.map((route) => route.upstreamModel)).toEqual([
      "anthropic/claude-sonnet-5",
      "anthropic/claude-opus-5",
      "anthropic/claude-haiku-4.5",
    ]);
  });

  it("uses the OpenAI-compatible v1 endpoint for Codex", () => {
    const preset = codexProviderPresets.find(
      (item) => item.name === "OrcaRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.auth).toEqual({ OPENAI_API_KEY: "" });
    expect(preset?.config).toContain('name = "orcarouter"');
    expect(preset?.config).toContain(
      'base_url = "https://api.orcarouter.ai/v1"',
    );
    expect(preset?.config).toContain('model = "openai/gpt-5.6-sol"');
    expect(preset?.config).toContain('wire_api = "responses"');
  });

  it("uses the Gemini-compatible endpoint for Gemini CLI", () => {
    const preset = geminiProviderPresets.find(
      (item) => item.name === "OrcaRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.baseURL).toBe("https://api.orcarouter.ai");
    expect(preset?.model).toBe("google/gemini-3.6-flash");

    const env = (preset?.settingsConfig as { env: Record<string, string> }).env;
    expect(env.GOOGLE_GEMINI_BASE_URL).toBe("https://api.orcarouter.ai");
    expect(env.GEMINI_MODEL).toBe("google/gemini-3.6-flash");
  });

  it("uses chat completions config for Hermes", () => {
    const preset = hermesProviderPresets.find(
      (item) => item.name === "OrcaRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig).toMatchObject({
      name: "orcarouter",
      base_url: "https://api.orcarouter.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
    });
    expect(preset?.settingsConfig.models?.map((model) => model.id)).toEqual([
      "anthropic/claude-opus-5",
      "anthropic/claude-sonnet-5",
      "anthropic/claude-haiku-4.5",
      "openai/gpt-5.6-sol",
      "google/gemini-3.6-flash",
    ]);
    expect(preset?.suggestedDefaults?.model).toEqual({
      default: "anthropic/claude-opus-5",
      provider: "orcarouter",
    });
  });

  it("uses OpenAI completions config for OpenClaw", () => {
    const preset = openclawProviderPresets.find(
      (item) => item.name === "OrcaRouter",
    );
    const [model] = preset?.settingsConfig.models ?? [];

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig.baseUrl).toBe("https://api.orcarouter.ai/v1");
    expect(preset?.settingsConfig.api).toBe("openai-completions");
    expect(model).toMatchObject({
      id: "anthropic/claude-opus-5",
      name: "Claude Opus 5",
      contextWindow: 1000000,
    });
    expect(preset?.templateValues?.apiKey?.placeholder).toBe("sk-orca-...");
    expect(preset?.suggestedDefaults?.model).toEqual({
      primary: "orcarouter/anthropic/claude-opus-5",
      fallbacks: ["orcarouter/anthropic/claude-sonnet-5"],
    });
  });

  it("uses the Anthropic SDK provider for OpenCode", () => {
    const preset = opencodeProviderPresets.find(
      (item) => item.name === "OrcaRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig.npm).toBe("@ai-sdk/anthropic");
    expect(preset?.settingsConfig.options?.baseURL).toBe(
      "https://api.orcarouter.ai/v1",
    );
    expect(preset?.settingsConfig.options?.apiKey).toBe("");
    expect(preset?.settingsConfig.models).toHaveProperty(
      "anthropic/claude-sonnet-5",
    );
    expect(preset?.settingsConfig.models).toHaveProperty(
      "anthropic/claude-opus-5",
    );
  });

  it("registers the OrcaRouter provider icon", () => {
    expect(hasIcon("orcarouter")).toBe(true);
  });
});
