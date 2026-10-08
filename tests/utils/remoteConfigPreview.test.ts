import { describe, expect, it } from "vitest";
import { maskRemoteSettingsConfig } from "@/utils/remoteConfigPreview";

const preview = (config: Record<string, unknown>) =>
  JSON.stringify(maskRemoteSettingsConfig(config));

describe("maskRemoteSettingsConfig", () => {
  it("masks the gateway token and auth headers inside Codex config.toml", () => {
    const text = preview({
      auth: { OPENAI_API_KEY: "sk-auth-secret" },
      config: `model_provider = "ccs"
model = "gpt-5"

[model_providers.ccs]
base_url = "http://127.0.0.1:23456/v1"
experimental_bearer_token = "ccsw-gateway-token"

[model_providers.ccs.http_headers]
Authorization = "Bearer header-secret"
X-Trace = "visible"
`,
    });

    expect(text).toContain("http://127.0.0.1:23456/v1");
    expect(text).toContain("gpt-5");
    expect(text).toContain("visible");
    expect(text).not.toContain("ccsw-gateway-token");
    expect(text).not.toContain("header-secret");
    expect(text).not.toContain("sk-auth-secret");
  });

  it("hides TOML that does not parse instead of showing it raw", () => {
    const text = preview({
      config: 'experimental_bearer_token = "leaked-token\n[broken',
    });
    expect(text).not.toContain("leaked-token");
  });

  it("masks JSON env credentials for Claude and Gemini", () => {
    const text = preview({
      env: {
        ANTHROPIC_AUTH_TOKEN: "claude-secret",
        GEMINI_API_KEY: "gemini-secret",
        ANTHROPIC_BASE_URL: "http://127.0.0.1:23456",
      },
    });
    expect(text).toContain("http://127.0.0.1:23456");
    expect(text).not.toContain("claude-secret");
    expect(text).not.toContain("gemini-secret");
  });
});
