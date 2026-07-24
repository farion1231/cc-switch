import { describe, expect, it } from "vitest";
import { parseDeepLinkConfigPreview } from "@/utils/deepLinkConfigPreview";

const encodeBase64 = (value: string) =>
  btoa(String.fromCharCode(...new TextEncoder().encode(value)));

const grokConfig = `[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "grok-4.5"
base_url = "https://relay.example/v1"
name = "Relay"
api_key = "secret-grok-key"
api_backend = "responses"
context_window = 500000
`;

describe("parseDeepLinkConfigPreview", () => {
  it("previews direct Grok Build TOML and masks its API key", () => {
    const preview = parseDeepLinkConfigPreview({
      app: "grokbuild",
      config: encodeBase64(grokConfig),
      configFormat: "toml",
    });

    expect(preview?.type).toBe("grokbuild");
    expect(preview?.tomlConfig).toContain("https://relay.example/v1");
    expect(preview?.tomlConfig).toContain("secr************");
    expect(preview?.tomlConfig).not.toContain("secret-grok-key");
  });

  it("previews wrapped Grok Build config JSON", () => {
    const preview = parseDeepLinkConfigPreview({
      app: "grokbuild",
      config: encodeBase64(JSON.stringify({ config: grokConfig })),
      configFormat: "json",
    });

    expect(preview?.type).toBe("grokbuild");
    expect(preview?.tomlConfig).toContain('default = "grok-4.5"');
    expect(preview?.tomlConfig).not.toContain("secret-grok-key");
  });

  it("masks authentication headers in nested TOML", () => {
    const config = `${grokConfig}
[mcp.servers.example]
url = "https://mcp.example"
headers = { Authorization = "Bearer top-secret", Cookie = "session=secret", credential = "credential-secret", auth = "auth-secret", safe_header = "visible" }
`;
    const preview = parseDeepLinkConfigPreview({
      app: "grokbuild",
      config: encodeBase64(config),
      configFormat: "toml",
    });

    expect(preview?.tomlConfig).not.toContain("Bearer top-secret");
    expect(preview?.tomlConfig).not.toContain("session=secret");
    expect(preview?.tomlConfig).not.toContain("credential-secret");
    expect(preview?.tomlConfig).not.toContain("auth-secret");
    expect(preview?.tomlConfig).toContain("visible");
  });

  it("also masks secrets in Codex TOML previews", () => {
    const preview = parseDeepLinkConfigPreview({
      app: "codex",
      config: encodeBase64(
        JSON.stringify({
          auth: { OPENAI_API_KEY: "secret-auth-key" },
          config: 'experimental_bearer_token = "secret-config-key"',
        }),
      ),
      configFormat: "json",
    });

    expect(preview?.type).toBe("codex");
    expect(preview?.tomlConfig).not.toContain("secret-config-key");
  });
});
