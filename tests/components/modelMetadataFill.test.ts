import { describe, expect, it } from "vitest";

import {
  fillCodexCatalogModel,
  fillHermesModel,
  fillKimiModel,
  fillOpenClawModel,
  fillOpenCodeModel,
  piPresetThinkingFor,
  piSendsReasoningEffort,
  piThinkingLevelMapFromEfforts,
} from "@/components/providers/forms/modelMetadataFill";
import type { KnownModelMetadata } from "@/lib/modelMetadata";

const metadata: KnownModelMetadata = {
  contextWindow: 262144,
  maxOutputTokens: 32768,
  reasoning: true,
  reasoningEfforts: ["max", "low", "turbo", "high"],
  inputModalities: ["text", "image", "video"],
  outputModalities: ["text"],
  cost: { input: 1, output: 4 },
};

const CODEX_LEVELS = ["none", "low", "medium", "high", "xhigh", "max"];

describe("fillCodexCatalogModel", () => {
  it("fills blank fields in Codex's level order", () => {
    expect(
      fillCodexCatalogModel(
        { model: "kimi-k2.6", contextWindow: "" },
        metadata,
        CODEX_LEVELS,
      ),
    ).toEqual({
      model: "kimi-k2.6",
      contextWindow: "262144",
      reasoningLevels: ["low", "high", "max"],
      inputModalities: ["text", "image"],
    });
  });

  it("never overwrites what the user already set", () => {
    const row = {
      model: "kimi-k2.6",
      contextWindow: "128000",
      reasoningLevels: ["high"],
      inputModalities: ["text"],
    };
    expect(fillCodexCatalogModel(row, metadata, CODEX_LEVELS)).toEqual(row);
  });

  it("never turns image input off for a text-only lookup", () => {
    expect(
      fillCodexCatalogModel(
        { model: "deepseek-v4.1-flash-1017", contextWindow: "" },
        { contextWindow: 1_000_000, inputModalities: ["text"] },
        CODEX_LEVELS,
      ),
    ).toEqual({
      model: "deepseek-v4.1-flash-1017",
      contextWindow: "1000000",
    });
  });
});

describe("fillOpenClawModel", () => {
  it("fills numbers and cost, and upgrades the default text-only input", () => {
    expect(
      fillOpenClawModel({ id: "m", name: "m", input: ["text"] }, metadata),
    ).toEqual({
      id: "m",
      name: "m",
      contextWindow: 262144,
      maxTokens: 32768,
      reasoning: true,
      input: ["text", "image"],
      cost: { input: 1, output: 4 },
    });
  });

  it("keeps user values and never downgrades capabilities", () => {
    const model = {
      id: "m",
      name: "m",
      contextWindow: 1000,
      maxTokens: 10,
      reasoning: true,
      input: ["text", "image"],
      cost: { input: 9, output: 9 },
    };
    expect(
      fillOpenClawModel(model, {
        ...metadata,
        reasoning: false,
        inputModalities: ["text"],
      }),
    ).toEqual(model);
  });
});

describe("fillHermesModel", () => {
  it("fills context_length and supports_vision only when blank", () => {
    expect(fillHermesModel({ id: "m" }, metadata)).toEqual({
      id: "m",
      context_length: 262144,
      supports_vision: true,
    });
    expect(
      fillHermesModel(
        { id: "m", context_length: 1, supports_vision: true },
        metadata,
      ),
    ).toEqual({ id: "m", context_length: 1, supports_vision: true });
  });

  it("only fills supports_vision towards true and keeps an explicit false", () => {
    expect(
      fillHermesModel({ id: "m" }, { inputModalities: ["text"] }),
    ).toEqual({ id: "m" });
    expect(
      fillHermesModel({ id: "m", supports_vision: false }, metadata),
    ).toEqual({ id: "m", context_length: 262144, supports_vision: false });
  });
});

describe("fillOpenCodeModel", () => {
  it("fills missing limits, modalities and reasoning", () => {
    expect(fillOpenCodeModel({ name: "m" }, metadata)).toEqual({
      name: "m",
      limit: { context: 262144, output: 32768 },
      reasoning: true,
      modalities: { input: ["text", "image", "video"], output: ["text"] },
    });
  });

  it("only fills reasoning towards true and keeps an explicit value", () => {
    expect(fillOpenCodeModel({ name: "m" }, { reasoning: false })).toEqual({
      name: "m",
    });
    expect(
      fillOpenCodeModel({ name: "m", reasoning: false }, { reasoning: true }),
    ).toEqual({ name: "m", reasoning: false });
  });

  it("only fills the missing half of limit", () => {
    expect(
      fillOpenCodeModel(
        {
          name: "m",
          limit: { context: 1000 },
          modalities: { input: ["text"] },
        },
        metadata,
      ),
    ).toEqual({
      name: "m",
      limit: { context: 1000, output: 32768 },
      modalities: { input: ["text"] },
      reasoning: true,
    });
  });

  it("leaves the model untouched when nothing is known", () => {
    expect(fillOpenCodeModel({ name: "m" }, {})).toEqual({ name: "m" });
  });
});

describe("fillKimiModel", () => {
  it("replaces a placeholder window and adds supported capabilities", () => {
    expect(
      fillKimiModel(
        { model: "m", max_context_size: 1, capabilities: ["tool_use"] },
        metadata,
        { replaceContext: true },
      ),
    ).toEqual({
      model: "m",
      max_context_size: 262144,
      capabilities: ["tool_use", "image_in", "thinking"],
    });
  });

  it("keeps a window the user set and never removes capabilities", () => {
    const model = {
      model: "m",
      max_context_size: 1000,
      capabilities: ["thinking", "video_in"],
    };
    expect(
      fillKimiModel(
        model,
        { contextWindow: 9, inputModalities: ["text"], reasoning: false },
        { replaceContext: false },
      ),
    ).toEqual(model);
  });
});

describe("piThinkingLevelMapFromEfforts", () => {
  it("follows Pi's own models.dev rule", () => {
    expect(piThinkingLevelMapFromEfforts(["high", "max"])).toEqual({
      off: null,
      minimal: null,
      low: null,
      medium: null,
      high: "high",
      xhigh: null,
      max: "max",
    });
    expect(piThinkingLevelMapFromEfforts(["none", "low"])).toMatchObject({
      off: "none",
      low: "low",
      high: null,
    });
  });

  it("generates nothing without a Pi level", () => {
    expect(piThinkingLevelMapFromEfforts(undefined)).toBeUndefined();
    expect(piThinkingLevelMapFromEfforts(["default"])).toBeUndefined();
  });
});

describe("piSendsReasoningEffort", () => {
  const base = {
    api: "openai-completions",
    baseUrl: "https://relay.example.com/v1",
    providerId: "relay",
    compat: {},
  };

  it("accepts Responses and plain Chat Completions endpoints", () => {
    expect(piSendsReasoningEffort({ ...base, api: "openai-responses" })).toBe(
      true,
    );
    expect(piSendsReasoningEffort(base)).toBe(true);
  });

  it("rejects protocols and endpoints with their own thinking format", () => {
    expect(piSendsReasoningEffort({ ...base, api: "anthropic-messages" })).toBe(
      false,
    );
    for (const baseUrl of [
      "https://api.deepseek.com",
      "https://open.bigmodel.cn/api/paas/v4",
      "https://openrouter.ai/api/v1",
      "https://api.moonshot.cn/v1",
    ]) {
      expect(piSendsReasoningEffort({ ...base, baseUrl })).toBe(false);
    }
    expect(piSendsReasoningEffort({ ...base, providerId: "zai" })).toBe(false);
  });

  it("lets explicit compat override the detection", () => {
    expect(
      piSendsReasoningEffort({
        ...base,
        baseUrl: "https://api.deepseek.com",
        compat: { thinkingFormat: "openai" },
      }),
    ).toBe(true);
    expect(
      piSendsReasoningEffort({
        ...base,
        compat: { supportsReasoningEffort: false },
      }),
    ).toBe(false);
  });
});

describe("piPresetThinkingFor", () => {
  const piThinking = {
    thinkingLevelMap: { low: "low", high: "high" },
    api: "openai-completions",
    baseUrl: "https://api.moonshot.cn/v1",
    compat: { supportsReasoningEffort: true, thinkingFormat: "openai" },
  };
  const effective = {
    api: "openai-completions",
    baseUrl: "https://api.moonshot.cn/v1",
    providerId: "my-moonshot",
    compat: {},
  };

  it("returns the map with the compat it still needs", () => {
    expect(
      piPresetThinkingFor(piThinking, {
        ...effective,
        compat: { thinkingFormat: "openai" },
      }),
    ).toEqual({
      map: { low: "low", high: "high" },
      missingCompat: { supportsReasoningEffort: true },
    });
  });

  it("skips the map under another protocol or conflicting compat", () => {
    expect(
      piPresetThinkingFor(piThinking, {
        ...effective,
        api: "anthropic-messages",
      }),
    ).toBeNull();
    expect(
      piPresetThinkingFor(piThinking, {
        ...effective,
        compat: { supportsReasoningEffort: false },
      }),
    ).toBeNull();
    expect(piPresetThinkingFor(undefined, effective)).toBeNull();
  });

  it("checks the behavior a preset relies on through Pi's own detection", () => {
    // OpenCode Go 的预设没写 compat，靠 Pi 按地址探测出会发 reasoning_effort。
    const relying = {
      thinkingLevelMap: { high: "high", max: "max" },
      api: "openai-completions",
      baseUrl: "https://opencode.ai/zen/go/v1",
    };
    const goEndpoint = {
      ...effective,
      baseUrl: "https://opencode.ai/zen/go/v1",
    };
    expect(piPresetThinkingFor(relying, goEndpoint)?.map).toEqual({
      high: "high",
      max: "max",
    });
    expect(
      piPresetThinkingFor(relying, {
        ...goEndpoint,
        compat: { supportsReasoningEffort: false },
      }),
    ).toBeNull();
  });
});
