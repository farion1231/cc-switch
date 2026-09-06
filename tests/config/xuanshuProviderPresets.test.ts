import { describe, expect, it } from "vitest";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";

const NAME = "XuanShu API";
const SITE = "https://www.xuanshuapi.com";

describe("XuanShu API provider preset", () => {
  it("points Claude Code at the root Anthropic endpoint without /v1", () => {
    const preset = providerPresets.find((item) => item.name === NAME);
    const env = (preset?.settingsConfig as { env: Record<string, string> }).env;

    expect(preset?.websiteUrl).toBe(SITE);
    expect(preset?.category).toBe("aggregator");
    expect(preset?.endpointCandidates).toEqual([SITE]);
    expect(env.ANTHROPIC_BASE_URL).toBe(SITE);
    expect(env.ANTHROPIC_AUTH_TOKEN).toBe("");
    expect(env.ANTHROPIC_API_KEY).toBeUndefined();
  });

  it("uses the Responses wire API for Codex without OpenAI auth", () => {
    const preset = codexProviderPresets.find((item) => item.name === NAME);

    expect(preset?.auth).toEqual({ OPENAI_API_KEY: "" });
    expect(preset?.endpointCandidates).toEqual([`${SITE}/v1`]);
    expect(preset?.config).toContain('model_provider = "xuanshu"');
    expect(preset?.config).toContain(`base_url = "${SITE}/v1"`);
    expect(preset?.config).toContain('wire_api = "responses"');
    expect(preset?.config).toContain("requires_openai_auth = false");
    expect(preset?.config).toContain("x-openai-actor-authorization");
  });

  it("uses the native Gemini endpoint at the root for Gemini CLI", () => {
    const preset = geminiProviderPresets.find((item) => item.name === NAME);
    const env = (preset?.settingsConfig as { env: Record<string, string> }).env;

    expect(preset?.baseURL).toBe(SITE);
    expect(env.GOOGLE_GEMINI_BASE_URL).toBe(SITE);
    expect(env.GEMINI_API_KEY).toBe("");
    expect(env.GEMINI_MODEL).toBe("gemini-3-pro-preview");
  });

  it("uses Anthropic Messages for Hermes", () => {
    const preset = hermesProviderPresets.find((item) => item.name === NAME);

    expect(preset?.settingsConfig).toMatchObject({
      name: "xuanshu",
      base_url: SITE,
      api_key: "",
      api_mode: "anthropic_messages",
    });
    expect(preset?.suggestedDefaults?.model).toEqual({
      default: "claude-opus-4-7",
      provider: "xuanshu",
    });
  });

  it("uses Anthropic Messages for OpenClaw", () => {
    const preset = openclawProviderPresets.find((item) => item.name === NAME);

    expect(preset?.settingsConfig).toMatchObject({
      baseUrl: SITE,
      apiKey: "",
      api: "anthropic-messages",
    });
    expect(preset?.suggestedDefaults?.model?.primary).toBe(
      "xuanshu/claude-opus-4-6",
    );
  });

  it("uses the Anthropic SDK against /v1 for OpenCode", () => {
    const preset = opencodeProviderPresets.find((item) => item.name === NAME);

    expect(preset?.settingsConfig.npm).toBe("@ai-sdk/anthropic");
    expect(preset?.settingsConfig.options?.baseURL).toBe(`${SITE}/v1`);
    expect(Object.keys(preset?.settingsConfig.models ?? {})).toContain(
      "claude-opus-4-7",
    );
  });
});
