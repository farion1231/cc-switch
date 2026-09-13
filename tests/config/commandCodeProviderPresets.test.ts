import { describe, expect, it } from "vitest";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { providerPresets } from "@/config/claudeProviderPresets";
import type { Provider } from "@/types";
import { providerNeedsRouting } from "@/utils/providerCapabilities";

const CODEX_BASE_URL = "https://api.commandcode.ai/provider/v1";
const CLAUDE_BASE_URL = "https://api.commandcode.ai/provider";

function asProvider(
  id: string,
  settingsConfig: Record<string, unknown>,
  apiFormat?: string,
): Provider {
  return {
    id,
    name: "Command Code",
    category: "third_party",
    settingsConfig,
    meta: apiFormat ? { apiFormat } : undefined,
  } as Provider;
}

describe("Command Code provider presets", () => {
  it("adds the Codex preset with a Chat-only non-Claude catalog", () => {
    const preset = codexProviderPresets.find(
      (item) => item.name === "Command Code",
    );

    expect(preset).toBeDefined();
    expect(preset?.apiFormat).toBe("openai_chat");
    expect(preset?.endpointCandidates).toEqual([CODEX_BASE_URL]);
    expect(preset?.config).toContain(`base_url = "${CODEX_BASE_URL}"`);
    expect(preset?.config).toContain('model = "deepseek/deepseek-v4.1-flash"');
    expect(preset?.modelCatalog?.map((model) => model.model)).toEqual([
      "deepseek/deepseek-v4.1-flash",
      "z-ai/glm-5.3-flash",
      "Qwen/Qwen3.8-Flash",
    ]);
    expect(
      preset?.modelCatalog?.some((model) => model.model.includes("claude")),
    ).toBe(false);
  });

  it("requires proxy takeover for the Codex Chat conversion", () => {
    const provider = asProvider(
      "command-code",
      {
        auth: { OPENAI_API_KEY: "" },
        config: `model_provider = "command_code"
model = "gpt-5.3-codex"

[model_providers.command_code]
base_url = "${CODEX_BASE_URL}"
wire_api = "responses"`,
      },
      "openai_chat",
    );

    expect(providerNeedsRouting("codex", provider)).toBe(true);
  });

  it("adds the Claude Code Anthropic preset with role model mapping", () => {
    const preset = providerPresets.find((item) => item.name === "Command Code");
    const env = (preset?.settingsConfig as { env?: Record<string, string> })
      ?.env;

    expect(preset).toBeDefined();
    expect(env).toMatchObject({
      ANTHROPIC_BASE_URL: CLAUDE_BASE_URL,
      ANTHROPIC_AUTH_TOKEN: "",
      ANTHROPIC_MODEL: "deepseek/deepseek-v4.1-flash",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "deepseek/deepseek-v4.1-flash",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "deepseek/deepseek-v4.1-flash",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "deepseek/deepseek-v4.1-flash",
    });
    expect(preset?.apiFormat).toBe("openai_chat");
    expect(preset?.modelsUrl).toBe(
      "https://api.commandcode.ai/provider/v1/models",
    );
    expect(preset?.apiKeyField).toBeUndefined();
  });

  it("routes Claude Code through Chat Completions for open models", () => {
    const preset = providerPresets.find((item) => item.name === "Command Code");
    const provider = asProvider(
      "command-code",
      preset?.settingsConfig as Record<string, unknown>,
      preset?.apiFormat,
    );

    expect(providerNeedsRouting("claude", provider)).toBe(true);
  });
});
