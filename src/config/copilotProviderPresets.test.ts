import { describe, expect, it } from "vitest";
import {
  COPILOT_ENV,
  compactCopilotEnv,
  copilotOfficialPreset,
  copilotProviderPresets,
  isCopilotByok,
} from "./copilotProviderPresets";

describe("copilotProviderPresets", () => {
  it("translates catalog presets into Copilot CLI BYOK variables", () => {
    expect(copilotProviderPresets.length).toBeGreaterThan(5);
    for (const preset of copilotProviderPresets) {
      const env = preset.settingsConfig.env;
      expect(env[COPILOT_ENV.baseUrl]).toMatch(/^https?:\/\//);
      expect(["openai", "anthropic"]).toContain(env[COPILOT_ENV.type]);
      if (env[COPILOT_ENV.type] === "anthropic") {
        expect(env[COPILOT_ENV.wireApi]).toBeUndefined();
      } else {
        expect(["completions", "responses"]).toContain(
          env[COPILOT_ENV.wireApi],
        );
      }
      expect(env[COPILOT_ENV.model]).toBeTruthy();
      expect(preset.models).toContain(env[COPILOT_ENV.model]);
    }
  });

  it("local presets get a placeholder key for servers without auth", () => {
    const local = copilotProviderPresets.filter((p) => p.category === "local");
    expect(local.length).toBeGreaterThan(0);
    for (const preset of local) {
      expect(preset.settingsConfig.env[COPILOT_ENV.apiKey]).toBeTruthy();
    }
  });

  it("official login is not BYOK and empty values are dropped", () => {
    expect(isCopilotByok(copilotOfficialPreset.settingsConfig)).toBe(false);
    expect(
      isCopilotByok({ env: { COPILOT_PROVIDER_BASE_URL: " http://x/v1 " } }),
    ).toBe(true);
    expect(
      compactCopilotEnv({
        COPILOT_MODEL: " gpt-5.4 ",
        COPILOT_PROVIDER_API_KEY: "",
      }),
    ).toEqual({ COPILOT_MODEL: "gpt-5.4" });
  });
});
