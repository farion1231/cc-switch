import { describe, expect, it } from "vitest";
import {
  getCopilotByokModelPreset,
  mergeCopilotByokModelOptions,
} from "@/lib/copilotByokModelPresets";
import type { ModelsDevResponse } from "@/lib/modelsDevPricing";

const modelsDev = {
  moonshotai: {
    name: "Moonshot AI",
    models: {
      "kimi-k3": {
        name: "Kimi K3",
        tool_call: true,
        reasoning: true,
        attachment: true,
        modalities: { input: ["text", "image", "video"], output: ["text"] },
        limit: { context: 1_048_576, output: 131_072 },
      },
      "kimi-k2.7-code": {
        name: "Kimi K2.7 Code",
        tool_call: true,
        reasoning: true,
        attachment: true,
        modalities: { input: ["text", "image", "video"], output: ["text"] },
        limit: { context: 262_144, output: 262_144 },
      },
      "kimi-k2.7-code-highspeed": {
        name: "Kimi K2.7 Code HighSpeed",
        tool_call: true,
        reasoning: true,
        attachment: true,
        modalities: { input: ["text", "image", "video"], output: ["text"] },
        limit: { context: 262_144, output: 262_144 },
      },
    },
  },
  anthropic: {
    name: "Anthropic",
    models: {
      "claude-sonnet-5": {
        name: "Claude Sonnet 5",
        tool_call: true,
        reasoning: true,
        modalities: { input: ["text", "image", "pdf"], output: ["text"] },
        limit: { context: 1_000_000, output: 128_000 },
      },
    },
  },
  openai: {
    name: "OpenAI",
    models: {
      "gpt-5.3-codex": {
        name: "GPT-5.3 Codex",
        tool_call: true,
        reasoning: true,
        modalities: { input: ["text", "image"], output: ["text"] },
        limit: { context: 400_000, input: 272_000, output: 128_000 },
      },
    },
  },
  alibaba: {
    name: "Alibaba",
    models: {
      "qwen3-coder-plus": {
        name: "Qwen3 Coder Plus",
        tool_call: true,
        reasoning: false,
        modalities: { input: ["text"], output: ["text"] },
        limit: { context: 1_048_576, output: 65_536 },
      },
    },
  },
  custom: {
    name: "Custom",
    models: {
      "no-tools-model": {
        name: "No Tools Model",
        tool_call: false,
      },
    },
  },
} satisfies ModelsDevResponse;

describe("Copilot BYOK model presets", () => {
  it("fills K3's documented capabilities and fixed request options", () => {
    const preset = getCopilotByokModelPreset(
      {
        providerName: "Custom Gateway",
        url: "https://gateway.example.com/v1",
        apiType: "chat-completions",
      },
      "k3",
    );

    expect(preset).toEqual(
      expect.objectContaining({
        toolCalling: true,
        vision: true,
        thinking: true,
        streaming: true,
        contextWindow: 1_000_000,
        supportsReasoningEffort: ["low", "high", "max"],
        reasoningEffortFormat: "chat-completions",
        modelOptions: { temperature: 1, top_p: 0.95 },
      }),
    );
  });

  it("defaults tool calling on for an unknown model without guessing other capabilities", () => {
    expect(
      getCopilotByokModelPreset(
        {
          providerName: "Custom",
          url: "https://api.example.com/v1",
          apiType: "chat-completions",
        },
        "my-model",
      ),
    ).toEqual({ toolCalling: true });
  });

  it("keeps an explicit models.dev tool-calling opt-out", () => {
    expect(
      getCopilotByokModelPreset(
        {
          providerName: "Custom",
          url: "https://api.example.com/v1",
          apiType: "chat-completions",
        },
        "no-tools-model",
        { modelsDev },
      ),
    ).toEqual({ toolCalling: false });
  });

  it("fills MiniMax M-series capabilities by model ID", () => {
    const context = {
      providerName: "Custom Gateway",
      url: "https://gateway.example.com/v1",
      apiType: "chat-completions" as const,
    };

    expect(getCopilotByokModelPreset(context, "MiniMax-M3")).toEqual(
      expect.objectContaining({
        toolCalling: true,
        vision: true,
        thinking: true,
        streaming: true,
        contextWindow: 1_000_000,
        maxOutputTokens: 524_288,
        modelOptions: { temperature: 1, top_p: 0.95 },
      }),
    );
    expect(
      getCopilotByokModelPreset(context, "MiniMax-M2.7-highspeed"),
    ).toEqual(
      expect.objectContaining({
        toolCalling: true,
        vision: false,
        thinking: true,
        streaming: true,
        contextWindow: 204_800,
        maxOutputTokens: 204_800,
        modelOptions: { temperature: 1, top_p: 0.9 },
      }),
    );
    expect(
      mergeCopilotByokModelOptions(
        "MiniMax-M3",
        { top_p: 0.7 },
        { temperature: 1, top_p: 0.95 },
      ),
    ).toEqual({ temperature: 1, top_p: 0.7 });
  });

  it("canonicalizes K3's fixed top_p constraint", () => {
    expect(
      mergeCopilotByokModelOptions(
        "k3",
        { top_p: 1, custom: true },
        { temperature: 1, top_p: 0.95 },
      ),
    ).toEqual({ temperature: 1, top_p: 0.95, custom: true });
  });

  it("resolves Kimi coding-plan aliases from the fetched display name", () => {
    const context = {
      providerName: "Kimi",
      url: "https://api.kimi.com/coding/v1",
      apiType: "chat-completions" as const,
    };

    expect(
      getCopilotByokModelPreset(context, "kimi-for-coding", {
        modelName: "K2.7 Coding",
        modelsDev,
      }),
    ).toEqual(
      expect.objectContaining({
        toolCalling: true,
        vision: true,
        thinking: true,
        streaming: true,
        contextWindow: 262_144,
        maxOutputTokens: 262_144,
        modelOptions: { temperature: 1, top_p: 0.95 },
      }),
    );
    expect(
      getCopilotByokModelPreset(context, "kimi-for-coding-highspeed", {
        modelName: "K2.7 Coding Highspeed",
        modelsDev,
      }),
    ).toEqual(
      expect.objectContaining({
        contextWindow: 262_144,
        maxOutputTokens: 262_144,
        modelOptions: { temperature: 1, top_p: 0.95 },
      }),
    );
  });

  it("honors the 256K K3 coding-plan endpoint variant", () => {
    expect(
      getCopilotByokModelPreset(
        {
          providerName: "Kimi",
          url: "https://api.kimi.com/coding/v1",
          apiType: "chat-completions",
        },
        "k3-256k",
        { modelName: "K3-256k", modelsDev },
      ),
    ).toEqual(
      expect.objectContaining({
        contextWindow: 262_144,
        maxOutputTokens: 131_072,
        supportsReasoningEffort: ["low", "high", "max"],
        modelOptions: { temperature: 1, top_p: 0.95 },
      }),
    );
  });

  it.each([
    {
      context: {
        providerName: "Anthropic",
        url: "https://api.anthropic.com/v1",
        apiType: "messages" as const,
      },
      id: "claude-sonnet-5",
      expected: { contextWindow: 1_000_000, vision: true, thinking: true },
    },
    {
      context: {
        providerName: "Custom Gateway",
        url: "https://gateway.example.com/v1",
        apiType: "responses" as const,
      },
      id: "openai/gpt-5.3-codex",
      expected: {
        contextWindow: 400_000,
        maxInputTokens: 272_000,
        maxOutputTokens: 128_000,
        toolCalling: true,
      },
    },
    {
      context: {
        providerName: "Alibaba",
        url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        apiType: "chat-completions" as const,
      },
      id: "qwen3-coder-plus",
      expected: {
        contextWindow: 1_048_576,
        vision: false,
        thinking: false,
        toolCalling: true,
      },
    },
  ])(
    "uses models.dev capabilities for common model $id",
    ({ context, id, expected }) => {
      expect(getCopilotByokModelPreset(context, id, { modelsDev })).toEqual(
        expect.objectContaining(expected),
      );
    },
  );
});
