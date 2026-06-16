import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import {
  applyGroupCommonConfig,
  getGroupCommonConfigCandidates,
} from "@/lib/provider-management/providerGroupCommonConfig";

describe("providerGroupCommonConfig", () => {
  it("applies group base URL and API key to a Claude provider without storing the secret in meta", () => {
    const source: Provider = {
      id: "source",
      name: "Source",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-source-secret",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "sonnet-a",
        },
      },
    };
    const target: Provider = {
      id: "target",
      name: "Target",
      settingsConfig: { env: {} },
    };

    const candidates = getGroupCommonConfigCandidates(source, "claude");
    const updated = applyGroupCommonConfig(target, source, "claude", [
      "baseUrl",
      "apiKey",
    ]);

    expect(candidates.baseUrl?.value).toBe("https://api.example.com");
    expect(candidates.apiKey?.displayValue).toBe("sk-sou...cret");
    expect((updated.settingsConfig.env as any).ANTHROPIC_BASE_URL).toBe(
      "https://api.example.com",
    );
    expect((updated.settingsConfig.env as any).ANTHROPIC_AUTH_TOKEN).toBe(
      "sk-source-secret",
    );
    expect(updated.meta?.groupCommonConfigEnabled).toEqual({
      baseUrl: true,
      apiKey: true,
    });
    expect(JSON.stringify(updated.meta)).not.toContain("sk-source-secret");
  });

  it("applies Codex base URL and model without dropping existing auth", () => {
    const source: Provider = {
      id: "source",
      name: "Source",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-source-secret" },
        config: `
model_provider = "custom"
model = "gpt-5.4"

[model_providers.custom]
base_url = "https://codex.example.com/v1"
`,
      },
    };
    const target: Provider = {
      id: "target",
      name: "Target",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-target-secret" },
        config: `model_provider = "custom"\nmodel = "old-model"\n`,
      },
    };

    const updated = applyGroupCommonConfig(target, source, "codex", [
      "baseUrl",
      "modelMapping",
    ]);

    expect((updated.settingsConfig.auth as any).OPENAI_API_KEY).toBe(
      "sk-target-secret",
    );
    expect(updated.settingsConfig.config).toContain(
      'base_url = "https://codex.example.com/v1"',
    );
    expect(updated.settingsConfig.config).toContain('model = "gpt-5.4"');
    expect(JSON.stringify(updated.meta)).not.toContain("sk-source-secret");
  });

  it("does not copy the Codex base URL when only applying the model", () => {
    const source: Provider = {
      id: "source",
      name: "Source",
      settingsConfig: {
        config: `
model_provider = "custom"
model = "gpt-5.4"

[model_providers.custom]
base_url = "https://source.example.com/v1"
`,
      },
    };
    const target: Provider = {
      id: "target",
      name: "Target",
      settingsConfig: {
        config: `
model_provider = "custom"
model = "old-model"

[model_providers.custom]
base_url = "https://target.example.com/v1"
`,
      },
    };

    const updated = applyGroupCommonConfig(target, source, "codex", [
      "modelMapping",
    ]);

    expect(updated.settingsConfig.config).toContain('model = "gpt-5.4"');
    expect(updated.settingsConfig.config).toContain(
      'base_url = "https://target.example.com/v1"',
    );
    expect(updated.settingsConfig.config).not.toContain(
      'base_url = "https://source.example.com/v1"',
    );
  });
});
