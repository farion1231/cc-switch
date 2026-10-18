import { describe, expect, it } from "vitest";
import { parse } from "smol-toml";
import {
  isCodexLiveWebSearchEnabled,
  setCodexLiveWebSearch,
} from "@/utils/providerConfigUtils";

describe("Codex live web search preference", () => {
  it.each([undefined, "disabled", "cached", "indexed", "live"])(
    "reads and changes %s without altering provider capabilities or profiles",
    (mode) => {
      const input = `${mode ? `web_search = "${mode}"\n` : ""}model_provider = "relay"
[model_providers.relay]
supports_standalone_web_search = false
[profiles.offline]
web_search = "disabled"
`;
      expect(isCodexLiveWebSearchEnabled(input)).toBe(mode === "live");
      const expected = parse(input);
      expected.web_search = "live";
      const enabled = setCodexLiveWebSearch(input, true);
      expect(parse(enabled)).toEqual(expected);
      expect(isCodexLiveWebSearchEnabled(enabled)).toBe(true);
      expect(setCodexLiveWebSearch(enabled, true)).toBe(enabled);
      expected.web_search = "cached";
      const disabled = setCodexLiveWebSearch(enabled, false);
      expect(parse(disabled)).toEqual(expected);
      expect(isCodexLiveWebSearchEnabled(disabled)).toBe(false);
    },
  );

  it("preserves comments and quoted keys", () => {
    const input = `# search preference\n'web_search' = 'cached' # keep\n`;
    expect(setCodexLiveWebSearch(input, true)).toBe(
      input.replace("'cached'", '"live"'),
    );
  });

  it.each([
    "",
    '[model_providers.relay] # provider\nname = "Relay"\n',
    '[[items]]\nweb_search = "disabled"\n',
    'instructions = """\nweb_search = "disabled"\n"""\n',
    '"web_search" = """cached"""\n',
  ])("safely writes the root key for unusual TOML: %s", (input) => {
    const expected = { ...parse(input), web_search: "live" };
    expect(parse(setCodexLiveWebSearch(input, true))).toEqual(expected);
  });

  it("does not overwrite invalid TOML", () => {
    const input = 'web_search = "unfinished';
    expect(setCodexLiveWebSearch(input, true)).toBe(input);
    expect(isCodexLiveWebSearchEnabled(input)).toBe(false);
  });
});
