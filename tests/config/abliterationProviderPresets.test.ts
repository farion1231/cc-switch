import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { hasIcon } from "@/icons/extracted";

const NAME = "Abliteration";
const WEBSITE_URL = "https://abliteration.ai";
const API_KEY_URL = "https://abliteration.ai/console";
const ANTHROPIC_BASE_URL = "https://api.abliteration.ai";
const OPENAI_BASE_URL = "https://api.abliteration.ai/v1";
const MODEL_ID = "abliterated-model";
const MODEL_NAME = "Abliterated Model";
const CONTEXT = 262144;
const LARGE_MODEL_ID = "abliterated-model-large";
const LARGE_MODEL_NAME = "Abliterated Large";
const LARGE_CONTEXT = 1000000;
const BRAND_COLOR = "#0A0A0A";

describe("Abliteration provider presets", () => {
  it("uses the Anthropic-compatible root endpoint for Claude", () => {
    const preset = providerPresets.find((item) => item.name === NAME);

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("third_party");
    expect(preset?.icon).toBe("abliteration");
    expect(preset?.iconColor).toBe(BRAND_COLOR);
    expect(preset?.endpointCandidates).toEqual([ANTHROPIC_BASE_URL]);

    const env = (preset?.settingsConfig as { env: Record<string, string> }).env;
    expect(env.ANTHROPIC_BASE_URL).toBe(ANTHROPIC_BASE_URL);
    expect(env.ANTHROPIC_AUTH_TOKEN).toBe("");
    expect(env.ANTHROPIC_MODEL).toBe(LARGE_MODEL_ID);
    // Haiku slot handles cheap background tasks → route to base; real work stays on large.
    expect(env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe(MODEL_ID);
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(LARGE_MODEL_ID);
    expect(env.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe(LARGE_MODEL_ID);
  });

  it("uses the OpenAI-compatible v1 endpoint for Codex", () => {
    const preset = codexProviderPresets.find((item) => item.name === NAME);

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("third_party");
    expect(preset?.icon).toBe("abliteration");
    expect(preset?.endpointCandidates).toEqual([OPENAI_BASE_URL]);
    expect(preset?.auth).toEqual({ OPENAI_API_KEY: "" });
    expect(preset?.config).toContain('name = "abliteration"');
    expect(preset?.config).toContain(`model = "${LARGE_MODEL_ID}"`);
    expect(preset?.config).toContain(`base_url = "${OPENAI_BASE_URL}"`);
    expect(preset?.config).toContain('wire_api = "responses"');

    const catalogModels = preset?.modelCatalog?.map((entry) => entry.model);
    expect(catalogModels).toEqual([LARGE_MODEL_ID, MODEL_ID]);
  });

  it("uses proxy Anthropic routing for Claude Desktop", () => {
    const preset = claudeDesktopProviderPresets.find(
      (item) => item.name === NAME,
    );

    expect(preset).toBeDefined();
    expect(preset?.websiteUrl).toBe(WEBSITE_URL);
    expect(preset?.apiKeyUrl).toBe(API_KEY_URL);
    expect(preset?.category).toBe("third_party");
    expect(preset?.baseUrl).toBe(ANTHROPIC_BASE_URL);
    expect(preset?.mode).toBe("proxy");
    expect(preset?.apiFormat).toBe("anthropic");
    expect(preset?.icon).toBe("abliteration");
    expect(preset?.modelRoutes?.length).toBeGreaterThan(0);
    expect(preset?.modelRoutes?.[0]?.upstreamModel).toBe(LARGE_MODEL_ID);
    expect(
      preset?.modelRoutes?.every(
        (route) => route.upstreamModel === LARGE_MODEL_ID,
      ),
    ).toBe(true);
  });

  it("uses chat completions config for Hermes", () => {
    const preset = hermesProviderPresets.find((item) => item.name === NAME);

    expect(preset).toBeDefined();
    expect(preset?.icon).toBe("abliteration");
    expect(preset?.category).toBe("third_party");
    expect(preset?.settingsConfig).toMatchObject({
      name: "abliteration",
      base_url: OPENAI_BASE_URL,
      api_key: "",
      api_mode: "chat_completions",
    });
    expect(preset?.settingsConfig.models).toEqual([
      {
        id: LARGE_MODEL_ID,
        name: LARGE_MODEL_NAME,
        context_length: LARGE_CONTEXT,
      },
      { id: MODEL_ID, name: MODEL_NAME, context_length: CONTEXT },
    ]);
    expect(preset?.settingsConfig.models?.[0]?.id).toBe(LARGE_MODEL_ID);
    expect(preset?.suggestedDefaults?.model).toEqual({
      default: LARGE_MODEL_ID,
      provider: "abliteration",
    });
  });

  it("uses OpenAI completions config for OpenClaw without hardcoded pricing", () => {
    const preset = openclawProviderPresets.find((item) => item.name === NAME);
    const [model] = preset?.settingsConfig.models ?? [];

    expect(preset).toBeDefined();
    expect(preset?.icon).toBe("abliteration");
    expect(preset?.category).toBe("third_party");
    expect(preset?.settingsConfig.baseUrl).toBe(OPENAI_BASE_URL);
    expect(preset?.settingsConfig.apiKey).toBe("");
    expect(preset?.settingsConfig.api).toBe("openai-completions");
    expect(model).toMatchObject({
      id: LARGE_MODEL_ID,
      name: LARGE_MODEL_NAME,
      contextWindow: LARGE_CONTEXT,
    });
    expect(model).not.toHaveProperty("cost");
    expect(preset?.settingsConfig.models).toContainEqual(
      expect.objectContaining({
        id: MODEL_ID,
        name: MODEL_NAME,
        contextWindow: CONTEXT,
      }),
    );
    expect(preset?.suggestedDefaults?.model).toEqual({
      primary: "abliteration/abliterated-model-large",
    });
    expect(preset?.suggestedDefaults?.modelCatalog).toEqual({
      "abliteration/abliterated-model-large": { alias: LARGE_MODEL_NAME },
      "abliteration/abliterated-model": { alias: MODEL_NAME },
    });
  });

  it("uses OpenAI-compatible config for OpenCode", () => {
    const preset = opencodeProviderPresets.find((item) => item.name === NAME);

    expect(preset).toBeDefined();
    expect(preset?.icon).toBe("abliteration");
    expect(preset?.category).toBe("third_party");
    expect(preset?.settingsConfig.npm).toBe("@ai-sdk/openai-compatible");
    expect(preset?.settingsConfig.options?.baseURL).toBe(OPENAI_BASE_URL);
    expect(preset?.settingsConfig.options?.apiKey).toBe("");
    expect(preset?.settingsConfig.models).toHaveProperty(LARGE_MODEL_ID);
    expect(preset?.settingsConfig.models).toHaveProperty(MODEL_ID);
  });

  it("registers the Abliteration provider icon", () => {
    expect(hasIcon("abliteration")).toBe(true);
  });
});
