import { describe, expect, it } from "vitest";
import { providerToDraft } from "@/components/pi/piProviderMapping";

describe("providerToDraft", () => {
  it("round-trips provider fields that are not directly editable in the form", () => {
    const draft = providerToDraft(
      {
        baseUrl: "https://api.example.com/v1",
        api: "anthropic-messages",
        apiKey: "$ANTHROPIC_API_KEY",
        headers: { "x-extra": "$EXTRA_TOKEN" },
        models: [
          {
            id: "claude-sonnet-4",
            name: "Claude Sonnet 4",
            reasoning: true,
            input: ["text", "image"],
            contextWindow: 200000,
            maxTokens: 64000,
            cost: {
              input: 3,
              output: 15,
              cacheRead: 0.3,
              cacheWrite: 3.75,
            },
          },
        ],
        compat: {
          supportsDeveloperRole: false,
          supportsReasoningEffort: true,
          supportsUsageInStreaming: true,
          maxTokensField: "max_tokens",
          thinkingFormat: "anthropic",
          supportsEagerToolInputStreaming: false,
          supportsLongCacheRetention: true,
          forceAdaptiveThinking: true,
          allowEmptySignature: true,
        },
        routing: { strategy: "least-latency" },
        rateLimits: { rpm: 60 },
      },
      { providerId: "anthropic" },
    );

    expect(draft).toEqual(
      expect.objectContaining({
        providerId: "anthropic",
        mode: "builtinOverride",
        template: "anthropicCompatible",
        baseUrl: "https://api.example.com/v1",
        api: "anthropic-messages",
        apiKey: { mode: "env", value: "ANTHROPIC_API_KEY" },
        headers: [{ key: "x-extra", value: "$EXTRA_TOKEN" }],
        advancedJson: {
          routing: { strategy: "least-latency" },
          rateLimits: { rpm: 60 },
        },
      }),
    );
    expect(draft.models[0]).toEqual(
      expect.objectContaining({
        id: "claude-sonnet-4",
        name: "Claude Sonnet 4",
        reasoning: true,
        input: ["text", "image"],
        contextWindow: 200000,
        maxTokens: 64000,
        cost: {
          input: 3,
          output: 15,
          cacheRead: 0.3,
          cacheWrite: 3.75,
        },
      }),
    );
    expect(draft.compat).toEqual({
      supportsDeveloperRole: false,
      supportsReasoningEffort: true,
      supportsUsageInStreaming: true,
      maxTokensField: "max_tokens",
      thinkingFormat: "anthropic",
      supportsEagerToolInputStreaming: false,
      supportsLongCacheRetention: true,
      forceAdaptiveThinking: true,
      allowEmptySignature: true,
    });
  });
});
