import { describe, expect, it } from "vitest";
import {
  normalizeCodexCatalogModelsForSave,
  normalizeCodexModelMappingForSave,
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

  it("normalizes upstream model mappings independently from the catalog", () => {
    expect(
      normalizeCodexModelMappingForSave([
        {
          rowId: "1",
          requestModel: " gpt-5.6-sol ",
          upstreamModel: " zy-gpt-5.6-sol ",
        },
        {
          rowId: "2",
          requestModel: "gpt-5.6-terra",
          upstreamModel: "zy-gpt-5.6-terra",
        },
        {
          rowId: "3",
          requestModel: "gpt-5.6-sol",
          upstreamModel: "duplicate-is-ignored",
        },
        { rowId: "4", requestModel: "", upstreamModel: "missing-source" },
        { rowId: "5", requestModel: "missing-target", upstreamModel: "" },
      ]),
    ).toEqual({
      "gpt-5.6-sol": "zy-gpt-5.6-sol",
      "gpt-5.6-terra": "zy-gpt-5.6-terra",
    });
  });

  it("omits empty upstream model mappings", () => {
    expect(
      normalizeCodexModelMappingForSave([
        { rowId: "1", requestModel: "gpt-5.6-sol", upstreamModel: " " },
      ]),
    ).toBeUndefined();
  });
});
