import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { hasIcon } from "@/icons/extracted";

const WEBSITE_URL = "https://subrouter.ai";
const API_KEY_URL = "https://subrouter.ai/register?aff=l3ri";

describe("SubRouter provider presets", () => {
  it("uses the Anthropic-compatible root endpoint for Claude", () => {
    const preset = providerPresets.find((item) => item.name === "SubRouter");

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.isPartner).toBe(true);
    expect(preset?.partnerPromotionKey).toBe("subrouter");
    expect(preset?.icon).toBe("subrouter");

    const env = (preset?.settingsConfig as { env: Record<string, string> }).env;
    expect(env.ANTHROPIC_BASE_URL).toBe("https://subrouter.ai");
    expect(env.ANTHROPIC_AUTH_TOKEN).toBe("");
  });

  it("uses the OpenAI-compatible v1 endpoint for Codex", () => {
    const preset = codexProviderPresets.find(
      (item) => item.name === "SubRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.endpointCandidates).toEqual(["https://subrouter.ai/v1"]);
    expect(preset?.auth).toEqual({ OPENAI_API_KEY: "" });
    expect(preset?.config).toContain('name = "subrouter"');
    expect(preset?.config).toContain('model = "gpt-5.5"');
    expect(preset?.config).toContain('base_url = "https://subrouter.ai/v1"');
    expect(preset?.config).toContain('wire_api = "responses"');
  });

  it("uses the Gemini-compatible v1beta endpoint for Gemini", () => {
    const preset = geminiProviderPresets.find(
      (item) => item.name === "SubRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.baseURL).toBe("https://subrouter.ai/v1beta");
    expect(preset?.endpointCandidates).toEqual(["https://subrouter.ai/v1beta"]);
    expect(preset?.model).toBe("gemini-3.5-flash");

    const env = (preset?.settingsConfig as { env: Record<string, string> }).env;
    expect(env.GOOGLE_GEMINI_BASE_URL).toBe("https://subrouter.ai/v1beta");
    expect(env.GEMINI_MODEL).toBe("gemini-3.5-flash");
  });

  it("uses OpenAI-compatible config for OpenCode", () => {
    const preset = opencodeProviderPresets.find(
      (item) => item.name === "SubRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig.npm).toBe("@ai-sdk/openai-compatible");
    expect(preset?.settingsConfig.options?.baseURL).toBe(
      "https://subrouter.ai/v1",
    );
    expect(preset?.settingsConfig.options?.apiKey).toBe("");
    expect(preset?.settingsConfig.models).toHaveProperty("gpt-5.5");
  });

  it("uses OpenAI completions config for OpenClaw without hardcoded pricing", () => {
    const preset = openclawProviderPresets.find(
      (item) => item.name === "SubRouter",
    );
    const [model] = preset?.settingsConfig.models ?? [];

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig.baseUrl).toBe("https://subrouter.ai/v1");
    expect(preset?.settingsConfig.api).toBe("openai-completions");
    expect(model).toMatchObject({
      id: "gpt-5.5",
      name: "GPT-5.5",
      contextWindow: 400000,
    });
    expect(model).not.toHaveProperty("cost");
    expect(preset?.suggestedDefaults?.model).toEqual({
      primary: "subrouter/gpt-5.5",
    });
  });

  it("uses chat completions config for Hermes", () => {
    const preset = hermesProviderPresets.find(
      (item) => item.name === "SubRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.settingsConfig).toMatchObject({
      name: "subrouter",
      base_url: "https://subrouter.ai/v1",
      api_key: "",
      api_mode: "chat_completions",
    });
    expect(preset?.suggestedDefaults?.model).toEqual({
      default: "gpt-5.5",
      provider: "subrouter",
    });
  });

  it("uses direct Anthropic routing for Claude Desktop", () => {
    const preset = claudeDesktopProviderPresets.find(
      (item) => item.name === "SubRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.baseUrl).toBe("https://subrouter.ai");
    expect(preset?.mode).toBe("direct");
    expect(preset?.apiFormat).toBe("anthropic");
    expect(preset?.modelRoutes?.length).toBeGreaterThan(0);
  });

  it("registers the SubRouter provider icon", () => {
    expect(hasIcon("subrouter")).toBe(true);
  });
});
