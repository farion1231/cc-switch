import { describe, expect, it } from "vitest";
import { providerToDraft } from "@/components/pi/piProviderMapping";

describe("providerToDraft", () => {
  it("preserves cost, all compat flags, and advancedJson when loading a provider", () => {
    const provider = {
      baseUrl: "https://api.example.com/v1",
      api: "anthropic-messages",
      apiKey: "$ANTHROPIC_API_KEY",
      headers: { "x-extra": "$EXTRA" },
      models: [
        {
          id: "claude-opus-4",
          name: "Claude Opus 4",
          reasoning: true,
          input: ["text", "image"],
          contextWindow: 200000,
          maxTokens: 32000,
          cost: { input: 15, output: 75, cacheRead: 1.5, cacheWrite: 18.75 },
        },
      ],
      compat: {
        supportsDeveloperRole: true,
        supportsReasoningEffort: false,
        supportsUsageInStreaming: true,
        maxTokensField: "max_tokens",
        thinkingFormat: "interleaved",
        supportsEagerToolInputStreaming: true,
        supportsLongCacheRetention: true,
        forceAdaptiveThinking: false,
        allowEmptySignature: true,
      },
      routing: { strategy: "least-latency" }, // unknown field the form doesn't model
    };

    const draft = providerToDraft(provider, { providerId: "myprov" });

    // cost fully preserved (previously dropped entirely on edit)
    expect(draft.models[0].cost).toEqual({
      input: 15,
      output: 75,
      cacheRead: 1.5,
      cacheWrite: 18.75,
    });

    // all 9 compat flags preserved (previously only 5 were restored)
    expect(draft.compat).toEqual({
      supportsDeveloperRole: true,
      supportsReasoningEffort: false,
      supportsUsageInStreaming: true,
      maxTokensField: "max_tokens",
      thinkingFormat: "interleaved",
      supportsEagerToolInputStreaming: true,
      supportsLongCacheRetention: true,
      forceAdaptiveThinking: false,
      allowEmptySignature: true,
    });

    // unknown field collected into advancedJson (previously null'd on edit)
    expect(draft.advancedJson).toEqual({
      routing: { strategy: "least-latency" },
    });

    // mode inferred (not hardcoded to custom) for a non-builtin id
    expect(draft.mode).toBe("custom");
    expect(draft.template).toBe("anthropicCompatible");
    // apiKey env mode preserved
    expect(draft.apiKey).toEqual({ mode: "env", value: "ANTHROPIC_API_KEY" });
    // headers preserved
    expect(draft.headers).toEqual([{ key: "x-extra", value: "$EXTRA" }]);
  });

  it("infers builtinOverride mode for a known builtin provider id", () => {
    const draft = providerToDraft(
      { api: "anthropic-messages", baseUrl: "https://x" },
      { providerId: "anthropic" },
    );
    expect(draft.mode).toBe("builtinOverride");
    expect(draft.template).toBe("anthropicCompatible");
  });

  it("preserves undefined compat flags instead of coercing them to false", () => {
    const draft = providerToDraft(
      { api: "openai-completions", compat: { supportsDeveloperRole: false } },
      { providerId: "x" },
    );
    expect(draft.compat?.supportsDeveloperRole).toBe(false);
    expect(draft.compat?.supportsReasoningEffort).toBeUndefined();
    expect(draft.compat?.supportsUsageInStreaming).toBeUndefined();
  });

  it("parses command apiKeys (kept with the ! prefix) and literal apiKeys", () => {
    const cmd = providerToDraft(
      { api: "openai-completions", apiKey: "!security find-generic-password" },
      { providerId: "x" },
    );
    expect(cmd.apiKey).toEqual({
      mode: "command",
      value: "!security find-generic-password",
    });

    const lit = providerToDraft(
      { api: "openai-completions", apiKey: "sk-literal" },
      { providerId: "x" },
    );
    expect(lit.apiKey).toEqual({ mode: "literal", value: "sk-literal" });
  });
});
