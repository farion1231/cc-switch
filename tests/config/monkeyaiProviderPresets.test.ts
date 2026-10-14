import { describe, expect, it } from "vitest";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { universalProviderPresets } from "@/config/universalProviderPresets";

describe("MonkeyAI presets", () => {
  it("points Claude at the public origin without /v1 and uses Bearer", () => {
    const preset = providerPresets.find((item) => item.name === "MonkeyAI");
    expect(preset?.apiKeyField).toBe("ANTHROPIC_AUTH_TOKEN");
    expect(preset?.apiFormat).toBe("anthropic");
    expect(preset?.settingsConfig).toEqual({
      env: {
        ANTHROPIC_BASE_URL: "https://${MONKEYAI_PUBLIC_HOST}",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "your-model-id",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "your-model-id",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "your-model-id",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "your-model-id",
      },
    });
  });

  it("points Codex at /v1 so wire_api responses hits /v1/responses", () => {
    const preset = codexProviderPresets.find((item) => item.name === "MonkeyAI");
    expect(preset?.apiFormat).toBe("openai_responses");
    expect(preset?.config).toContain('base_url = "https://monkeyai.example.com/v1"');
    expect(preset?.config).toContain('wire_api = "responses"');
    expect(preset?.config).not.toContain("/v1/v1");
  });

  it("does not enable Gemini on the universal preset", () => {
    const preset = universalProviderPresets.find(
      (item) => item.providerType === "monkeyai",
    );
    expect(preset?.defaultApps).toEqual({
      claude: true,
      codex: true,
      gemini: false,
    });
  });
});
