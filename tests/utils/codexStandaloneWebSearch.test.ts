import { describe, expect, it } from "vitest";
import { parse } from "smol-toml";
import {
  isCodexStandaloneWebSearchEnabled,
  setCodexStandaloneWebSearch,
} from "@/utils/providerConfigUtils";

describe("Codex standalone web search capability", () => {
  const config = `# Keep this comment
model_provider = "relay"
web_search = "disabled"
[model_providers.relay]
name = "Relay"
base_url = "https://relay.example/v1"
[model_providers.other]
supports_standalone_web_search = true
[mcp_servers.search]
command = "search"
`;

  it("toggles only the selected provider and preserves the global search mode", () => {
    expect(isCodexStandaloneWebSearchEnabled(config)).toBe(false);
    const enabled = setCodexStandaloneWebSearch(config, true);
    const expected = parse(config) as any;
    expected.model_providers.relay.supports_standalone_web_search = true;
    expect(parse(enabled)).toEqual(expected);
    expect(enabled).toContain("# Keep this comment");
    expect(isCodexStandaloneWebSearchEnabled(enabled)).toBe(true);
    expect(setCodexStandaloneWebSearch(enabled, true)).toBe(enabled);
    const disabled = setCodexStandaloneWebSearch(enabled, false);
    expected.model_providers.relay.supports_standalone_web_search = false;
    expect(parse(disabled)).toEqual(expected);
    expect(isCodexStandaloneWebSearchEnabled(disabled)).toBe(false);
  });

  it("preserves quoted table names, quoted keys and inline comments", () => {
    const input = `model_provider = 'my.relay'
[ model_providers . 'my.relay' ]
name = "Relay"
"supports_standalone_web_search" = false # capability
`;
    expect(setCodexStandaloneWebSearch(input, true)).toBe(
      input.replace("= false # capability", "= true # capability"),
    );
  });

  it.each([
    '[model_providers.relay]\nname = "Relay"\nnotes = """\nsupports_standalone_web_search = false\n"""',
  ])("handles TOML syntax without modifying unrelated values: %s", (body) => {
    const input = `model_provider = "relay"\n${body}\n`;
    const expected = parse(input) as any;
    expected.model_providers.relay.supports_standalone_web_search = true;
    expect(parse(setCodexStandaloneWebSearch(input, true))).toEqual(expected);
  });

  it.each([
    'model_providers.relay = { name = "Relay", supports_standalone_web_search = false }',
    '[model_providers.relay] # provider\nname = "Relay"',
    "[model_providers.relay]\nname = \"Relay\"\n[mcp_servers.fs] # tools\ncommand = 'fs'\nstartup_timeout_sec = 30.0",
  ])(
    "preserves every byte when the table cannot be safely edited: %s",
    (body) => {
      const input = `# “Keep this comment”\nmodel_provider = "relay"\n${body}\n`;
      for (const enabled of [true, false]) {
        expect(setCodexStandaloneWebSearch(input, enabled)).toBe(input);
        const crlf = input.replace(/\n/g, "\r\n");
        expect(setCodexStandaloneWebSearch(crlf, enabled)).toBe(crlf);
      }
    },
  );

  it("inserts the capability without changing any unrelated text", () => {
    const input = `# “Keep this comment”\nmodel_provider = "relay"\n[model_providers.relay]\nname = 'Relay'\n\n[mcp_servers.fs]\nstartup_timeout_sec = 30.0 # keep float\n`;
    expect(setCodexStandaloneWebSearch(input, true)).toBe(
      input.replace(
        "name = 'Relay'",
        "name = 'Relay'\nsupports_standalone_web_search = true",
      ),
    );
  });

  it.each([
    "",
    'model_provider = "openai"',
    'model_provider = "relay"',
    'model_provider = "relay"\n[model_providers.relay]\nname = "unfinished',
  ])(
    "leaves missing, built-in or invalid provider configurations untouched",
    (input) => {
      expect(setCodexStandaloneWebSearch(input, true)).toBe(input);
      expect(isCodexStandaloneWebSearchEnabled(input)).toBe(false);
    },
  );
});
