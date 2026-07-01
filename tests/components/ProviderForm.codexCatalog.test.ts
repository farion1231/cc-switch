import { describe, expect, it } from "vitest";
import {
  normalizeCodexCatalogModelsForSave,
  resolveClaudeManagedProviderType,
} from "@/components/providers/forms/ProviderForm";

describe("ProviderForm Codex catalog helpers", () => {
  it("normalizes catalog rows and removes empty or duplicate models", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        { model: " deepseek-v4-flash ", displayName: " DeepSeek " },
        { model: "deepseek-v4-flash", displayName: "Duplicate" },
        { model: "", displayName: "Empty" },
        { model: "kimi-k2", contextWindow: "128000 tokens" },
      ]),
    ).toEqual([
      { model: "deepseek-v4-flash", displayName: "DeepSeek" },
      { model: "kimi-k2", contextWindow: 128000 },
    ]);
  });

  it("preserves native-profile overrides (parallel tool calls + input modalities + base instructions)", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        {
          model: "MiniMax-M3",
          displayName: "MiniMax-M3",
          contextWindow: 1000000,
          supportsParallelToolCalls: true,
          inputModalities: ["text", "image"],
          baseInstructions:
            "  You are Codex, a coding agent based on MiniMax-M3.  ",
        },
        // false must be preserved (not dropped as falsy); empty modalities dropped;
        // empty/whitespace baseInstructions dropped
        {
          model: "mimo-v2.5-pro",
          supportsParallelToolCalls: false,
          inputModalities: [],
          baseInstructions: "   ",
        },
      ]),
    ).toEqual([
      {
        model: "MiniMax-M3",
        displayName: "MiniMax-M3",
        contextWindow: 1000000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        baseInstructions: "You are Codex, a coding agent based on MiniMax-M3.",
      },
      { model: "mimo-v2.5-pro", supportsParallelToolCalls: false },
    ]);
  });

  it("infers Codex OAuth provider type from the ChatGPT Codex base URL", () => {
    expect(
      resolveClaudeManagedProviderType({
        baseUrl: "https://chatgpt.com/backend-api/codex",
      }),
    ).toBe("codex_oauth");

    expect(
      resolveClaudeManagedProviderType({
        baseUrl:
          "https://relay.example/v1?upstream=https://chatgpt.com/backend-api/codex",
      }),
    ).toBeUndefined();
  });

  it("infers Codex OAuth provider type from Claude settingsConfig fallback URLs", () => {
    for (const settingsConfig of [
      { base_url: "https://chatgpt.com/backend-api/codex" },
      { baseURL: "https://chatgpt.com/backend-api/codex" },
      { apiEndpoint: "https://chatgpt.com/backend-api/codex" },
      { apiEndpoint: { url: "https://chatgpt.com/backend-api/codex" } },
    ]) {
      expect(
        resolveClaudeManagedProviderType({
          baseUrl: "",
          settingsConfig: JSON.stringify(settingsConfig),
        }),
      ).toBe("codex_oauth");
    }
  });
});
