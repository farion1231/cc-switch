import { describe, expect, it } from "vitest";
import {
  codexApiFormatFromWireApi,
  isCodexAnthropicWireApi,
  isCodexRemoteCompactionEnabled,
  setCodexRemoteCompaction,
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
