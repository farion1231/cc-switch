import { describe, expect, it } from "vitest";
import {
  getModelFromConfig,
  isCodexRemoteCompactionEnabled,
  setCodexRemoteCompaction,
} from "./providerConfigUtils";

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

describe("getModelFromConfig", () => {
  it("reads the explicit Claude upstream model", () => {
    const config = { env: { ANTHROPIC_MODEL: "claude-sonnet-4-5" } };
    expect(getModelFromConfig(config, "claude")).toBe("claude-sonnet-4-5");
  });

  it("surfaces a cross-model mapping (the whole point of failover)", () => {
    // A Claude provider whose upstream is actually DeepSeek via Anthropic-compatible API.
    const config = { env: { ANTHROPIC_MODEL: "deepseek-chat" } };
    expect(getModelFromConfig(config, "claude")).toBe("deepseek-chat");
  });

  it("falls back to the SONNET default when ANTHROPIC_MODEL is absent", () => {
    const config = { env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-4.6" } };
    expect(getModelFromConfig(config, "claude")).toBe("glm-4.6");
  });

  it("reads Codex and Gemini model keys", () => {
    expect(getModelFromConfig({ env: { CODEX_MODEL: "gpt-5" } }, "codex")).toBe(
      "gpt-5",
    );
    expect(
      getModelFromConfig({ env: { GEMINI_MODEL: "gemini-2.5-pro" } }, "gemini"),
    ).toBe("gemini-2.5-pro");
  });

  it("accepts a JSON string as well as an object", () => {
    const json = JSON.stringify({ env: { ANTHROPIC_MODEL: "qwen-max" } });
    expect(getModelFromConfig(json, "claude")).toBe("qwen-max");
  });

  it("returns empty string for missing/invalid/empty config", () => {
    expect(getModelFromConfig(undefined, "claude")).toBe("");
    expect(getModelFromConfig("not json", "claude")).toBe("");
    expect(getModelFromConfig({ env: {} }, "claude")).toBe("");
    expect(getModelFromConfig({ env: { ANTHROPIC_MODEL: "" } }, "claude")).toBe(
      "",
    );
  });
});
