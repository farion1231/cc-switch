import { describe, expect, it } from "vitest";
import { parseDeepLinkConfigPreview } from "@/utils/deepLinkConfigPreview";

const encodeBase64 = (value: string) =>
  btoa(String.fromCharCode(...new TextEncoder().encode(value)));

const encodeUrlSafeBase64WithoutPadding = (value: string) =>
  encodeBase64(value)
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");

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

  it("previews URL-safe, unpadded, and space-normalized Grok Build TOML", () => {
    let config = "";
    for (let shift = 0; shift < 12; shift += 1) {
      config = `${grokConfig}\n# ${"p".repeat(shift)}🚀`;
      const candidate = encodeBase64(config);
      if (candidate.includes("+") && /=+$/.test(candidate)) break;
    }
    const standard = encodeBase64(config);
    const encoded = encodeUrlSafeBase64WithoutPadding(config);
    const encodedWithSpaces = standard.replace(/\+/g, " ");
    expect(standard).toContain("+");
    expect(standard).toMatch(/=+$/);
    expect(encoded).toContain("-");
    expect(encoded).not.toMatch(/=$/);
    expect(encodedWithSpaces).toContain(" ");

    const urlSafePreview = parseDeepLinkConfigPreview({
      app: "grokbuild",
      config: encoded,
      configFormat: "toml",
    });
    const spaceNormalizedPreview = parseDeepLinkConfigPreview({
      app: "grokbuild",
      config: encodedWithSpaces,
      configFormat: "toml",
    });

    for (const preview of [urlSafePreview, spaceNormalizedPreview]) {
      expect(preview?.type).toBe("grokbuild");
      expect(preview?.tomlConfig).toContain("https://relay.example/v1");
      expect(preview?.tomlConfig).not.toContain("secret-grok-key");
    }
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

  it("inherits sensitivity through nested tables and arrays of tables", () => {
    const config = `${grokConfig}
[model."grok-4.5".auth]
value = "nested-auth-secret"
short = "tiny"
empty = ""
nested = { value = "inline-nested-secret" }

[[model."grok-4.5".credentials]]
value = "array-table-secret"
`;
    const preview = parseDeepLinkConfigPreview({
      app: "grokbuild",
      config: encodeBase64(config),
      configFormat: "toml",
    });

    expect(preview?.tomlConfig).not.toContain("nested-auth-secret");
    expect(preview?.tomlConfig).not.toContain("inline-nested-secret");
    expect(preview?.tomlConfig).not.toContain("array-table-secret");
    expect(preview?.tomlConfig).toContain("nest************");
    expect(preview?.tomlConfig).toContain("inli************");
    expect(preview?.tomlConfig).toContain("arra************");
    expect(preview?.tomlConfig).toContain('short = "****"');
    expect(preview?.tomlConfig).toContain('empty = ""');
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

it("previews agy wrapped and flat configs without unrelated native options", () => {
  for (const config of [
    {
      env: {
        GEMINI_API_KEY: "fixture",
        GEMINI_MODEL: "legacy",
        NODE_OPTIONS: "ignored",
      },
      config: { model: { name: "native" }, arbitrary: "ignored" },
    },
    {
      GEMINI_API_KEY: "fixture",
      GEMINI_MODEL: "native",
      NODE_OPTIONS: "ignored",
    },
  ]) {
    const preview = parseDeepLinkConfigPreview({
      app: "agy",
      config: encodeBase64(JSON.stringify(config)),
    });
    expect(preview?.type).toBe("agy");
    expect(preview?.model).toBe("native");
    expect(preview?.env).toEqual({ GEMINI_API_KEY: "fixture" });
    expect(JSON.stringify(preview)).not.toContain("ignored");
  }
});

it("previews Copilot providers.json entries and legacy env configs", () => {
  const preview = parseDeepLinkConfigPreview({
    app: "copilot",
    config: encodeBase64(
      JSON.stringify({
        provider: {
          type: "openai",
          baseUrl: "https://relay.example/v1",
          apiKey: "fixture",
          headers: { "X-Ignored": "ignored" },
        },
        models: [{ id: "m1" }, { id: "m2" }],
      }),
    ),
  });
  expect(preview).toEqual({
    type: "copilot",
    env: { baseUrl: "https://relay.example/v1", apiKey: "fixture" },
    model: "m1, m2",
  });

  const legacy = parseDeepLinkConfigPreview({
    app: "copilot",
    config: encodeBase64(
      JSON.stringify({
        env: {
          COPILOT_PROVIDER_BASE_URL: "https://relay.example/v1",
          COPILOT_PROVIDER_API_KEY: "fixture",
          COPILOT_MODEL: "m1",
        },
      }),
    ),
  });
  expect(legacy?.model).toBe("m1");
  expect(legacy?.env).toEqual({
    COPILOT_PROVIDER_API_KEY: "fixture",
    COPILOT_PROVIDER_BASE_URL: "https://relay.example/v1",
  });
});

it("shows the effective Copilot credential and ordered model IDs after overrides", () => {
  const config = encodeBase64(
    JSON.stringify({
      provider: {
        baseUrl: "https://example.org",
        bearerToken: "token",
        apiKey: "shadowed",
      },
      models: [{ id: "one" }, { id: "two" }],
    }),
  );
  expect(parseDeepLinkConfigPreview({ app: "copilot", config })?.env).toEqual({
    baseUrl: "https://example.org",
    bearerToken: "token",
  });
  expect(
    parseDeepLinkConfigPreview({
      app: "copilot",
      config,
      apiKey: "url-key",
      model: "two",
    }),
  ).toEqual({
    type: "copilot",
    env: { baseUrl: "https://example.org", apiKey: "url-key" },
    model: "two, one",
  });
});
