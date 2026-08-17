import { describe, expect, it } from "vitest";
import { providerPresets } from "./claudeProviderPresets";
import { codexProviderPresets } from "./codexProviderPresets";

describe("OrcaRouter presets", () => {
  it("registers exactly one Claude Code preset", () => {
    const orca = providerPresets.filter((p) => p.name === "OrcaRouter");
    expect(orca).toHaveLength(1);
    expect(orca[0].settingsConfig).toEqual({
      env: {
        ANTHROPIC_BASE_URL: "https://api.orcarouter.ai",
        ANTHROPIC_AUTH_TOKEN: "",
        ANTHROPIC_MODEL: "anthropic/claude-sonnet-5",
        ANTHROPIC_DEFAULT_HAIKU_MODEL: "anthropic/claude-haiku-4.5",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "anthropic/claude-sonnet-5",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "anthropic/claude-opus-5",
      },
    });
  });

  it("registers exactly one Codex preset", () => {
    const orca = codexProviderPresets.filter((p) => p.name === "OrcaRouter");
    expect(orca).toHaveLength(1);
    expect(orca[0].config).toContain(
      'base_url = "https://api.orcarouter.ai/v1"',
    );
    expect(orca[0].config).toContain('model = "anthropic/claude-sonnet-5"');
    expect(orca[0].auth).toEqual({ OPENAI_API_KEY: "" });
  });
});
