import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";

describe("TokenRouter provider preset", () => {
  it("uses the Anthropic-compatible root endpoint for Claude", () => {
    const preset = providerPresets.find((item) => item.name === "TokenRouter");
    const env = (
      preset?.settingsConfig as {
        env: Record<string, string>;
      }
    ).env;

    expect(preset?.websiteUrl).toBe("https://tokenrouter.com");
    expect(preset?.category).toBe("aggregator");
    expect(preset?.endpointCandidates).toEqual(["https://api.tokenrouter.com"]);
    expect(env.ANTHROPIC_BASE_URL).toBe("https://api.tokenrouter.com");
    expect(env.ANTHROPIC_AUTH_TOKEN).toBe("");
    expect(env.ANTHROPIC_MODEL).toBe("anthropic/claude-sonnet-5");
    expect(env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe("anthropic/claude-haiku-4.5");
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("anthropic/claude-sonnet-5");
    expect(env.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe("anthropic/claude-opus-4.8");
  });

  it("uses Anthropic proxy routes for Claude Desktop", () => {
    const preset = claudeDesktopProviderPresets.find(
      (item) => item.name === "TokenRouter",
    );

    expect(preset?.baseUrl).toBe("https://api.tokenrouter.com");
    expect(preset?.mode).toBe("proxy");
    expect(preset?.apiFormat).toBe("anthropic");
    expect(preset?.modelRoutes?.map((route) => route.upstreamModel)).toEqual([
      "anthropic/claude-sonnet-5",
      "anthropic/claude-opus-4.8",
      "anthropic/claude-haiku-4.5",
    ]);
  });

  it("uses the OpenAI Responses-compatible endpoint for Codex", () => {
    const preset = codexProviderPresets.find(
      (item) => item.name === "TokenRouter",
    );

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe("https://tokenrouter.com");
    expect(preset?.category).toBe("aggregator");
    expect(preset?.endpointCandidates).toEqual([
      "https://api.tokenrouter.com/v1",
    ]);
    expect(preset?.auth).toEqual({ OPENAI_API_KEY: "" });
    expect(preset?.config).toContain('model_provider = "custom"');
    expect(preset?.config).toContain('model = "gpt-5.5"');
    expect(preset?.config).toContain("[model_providers.custom]");
    expect(preset?.config).toContain('name = "tokenrouter"');
    expect(preset?.config).toContain(
      'base_url = "https://api.tokenrouter.com/v1"',
    );
    expect(preset?.config).toContain('wire_api = "responses"');
    expect(preset?.config).toContain("requires_openai_auth = true");
  });

  it("uses the OpenAI-compatible endpoint for OpenCode", () => {
    const preset = opencodeProviderPresets.find(
      (item) => item.name === "TokenRouter",
    );

    expect(preset?.settingsConfig.npm).toBe("@ai-sdk/openai-compatible");
    expect(preset?.settingsConfig.options?.baseURL).toBe(
      "https://api.tokenrouter.com/v1",
    );
    expect(preset?.settingsConfig.models).toEqual({
      "gpt-5.5": { name: "GPT-5.5" },
    });
  });

  it("uses Chat Completions for Hermes", () => {
    const preset = hermesProviderPresets.find(
      (item) => item.name === "TokenRouter",
    );

    expect(preset?.settingsConfig).toMatchObject({
      name: "tokenrouter",
      base_url: "https://api.tokenrouter.com/v1",
      api_key: "",
      api_mode: "chat_completions",
    });
    expect(preset?.suggestedDefaults?.model).toEqual({
      default: "gpt-5.5",
      provider: "tokenrouter",
    });
  });

  it("uses OpenAI Completions for OpenClaw", () => {
    const preset = openclawProviderPresets.find(
      (item) => item.name === "TokenRouter",
    );

    expect(preset?.settingsConfig).toMatchObject({
      baseUrl: "https://api.tokenrouter.com/v1",
      apiKey: "",
      api: "openai-completions",
    });
    expect(preset?.suggestedDefaults?.model?.primary).toBe(
      "tokenrouter/gpt-5.5",
    );
  });

  it("does not offer a broken Gemini preset without a native v1beta endpoint", () => {
    expect(
      geminiProviderPresets.some((item) => item.name === "TokenRouter"),
    ).toBe(false);
  });
});
