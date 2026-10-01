import { describe, expect, it } from "vitest";

import {
  resolveModelMetadata,
  type KnownModelMetadata,
  type PresetModelSource,
} from "@/lib/modelMetadata";
import type { ModelsDevResponse } from "@/lib/modelsDev";

const modelsDev: ModelsDevResponse = {
  zhipuai: {
    id: "zhipuai",
    api: "https://open.bigmodel.cn/api/paas/v4",
    models: {
      "glm-5": {
        id: "glm-5",
        limit: { context: 204800, output: 131072 },
        reasoning: true,
        reasoning_options: [{ type: "toggle" }],
        modalities: { input: ["text"], output: ["text"] },
        cost: { input: 1, output: 3.2 },
      },
    },
  },
  zai: {
    id: "zai",
    api: "https://api.z.ai/api/paas/v4",
    models: {
      "glm-5": { id: "glm-5", limit: { context: 200000, output: 128000 } },
    },
  },
  "zai-coding-plan": {
    id: "zai-coding-plan",
    api: "https://api.z.ai/api/coding/paas/v4",
    models: {
      "glm-5": {
        id: "glm-5",
        limit: { context: 202752, output: 98304 },
        cost: { input: 0, output: 0 },
      },
    },
  },
  "opencode-go": {
    id: "opencode-go",
    api: "https://opencode.ai/zen/go/v1",
    models: {
      "glm-5.3": {
        id: "glm-5.3",
        limit: { context: 1000000, output: 131072 },
        reasoning_options: [{ type: "effort", values: ["low", "high", "max"] }],
      },
    },
  },
  openrouter: {
    id: "openrouter",
    api: "https://openrouter.ai/api/v1",
    models: {
      "z-ai/glm-5": {
        id: "z-ai/glm-5",
        limit: { context: 202800 },
        canonical_model_id: "zhipuai/glm-5",
      },
      "someone/kimi-k9": {
        id: "someone/kimi-k9",
        limit: { context: 65536 },
        canonical_model_id: "moonshotai/kimi-k9",
      },
    },
  },
  moonshotai: {
    id: "moonshotai",
    api: "https://api.moonshot.ai/v1",
    models: {
      "kimi-k9": { id: "kimi-k9", limit: { context: 262144 } },
    },
  },
  other: {
    id: "other",
    models: {
      "kimi-k9": {
        id: "kimi-k9",
        limit: { context: 100000 },
        canonical_model_id: "elsewhere/kimi-k9",
      },
    },
  },
  elsewhere: {
    id: "elsewhere",
    models: { "kimi-k9": { id: "kimi-k9", limit: { context: 50000 } } },
  },
};

const preset = (
  baseUrl: string,
  models: Record<string, KnownModelMetadata>,
): PresetModelSource => ({ baseUrl, models: new Map(Object.entries(models)) });

describe("resolveModelMetadata", () => {
  it("uses the provider recognized by its API address", () => {
    expect(
      resolveModelMetadata("glm-5.3", {
        baseUrl: "https://opencode.ai/zen/go/v1/",
        modelsDev,
      }),
    ).toMatchObject({
      contextWindow: 1000000,
      maxOutputTokens: 131072,
      reasoningEfforts: ["low", "high", "max"],
    });
  });

  it("prefers the provider whose path matches the address best", () => {
    expect(
      resolveModelMetadata("glm-5", {
        baseUrl: "https://api.z.ai/api/coding/paas/v4",
        modelsDev,
      })?.contextWindow,
    ).toBe(202752);
    expect(
      resolveModelMetadata("glm-5", {
        baseUrl: "https://api.z.ai/api/paas/v4",
        modelsDev,
      })?.contextWindow,
    ).toBe(200000);
  });

  it("matches ids case-insensitively and across namespaces", () => {
    expect(
      resolveModelMetadata("Z-AI/GLM-5", {
        baseUrl: "https://openrouter.ai/api/v1",
        modelsDev,
      })?.contextWindow,
    ).toBe(202800);
    expect(
      resolveModelMetadata("zai-org/glm-5", {
        baseUrl: "https://open.bigmodel.cn/api/paas/v4",
        modelsDev,
      })?.contextWindow,
    ).toBe(204800);
  });

  it("falls back to the vendor entry without carrying its price", () => {
    const metadata = resolveModelMetadata("glm-5", {
      baseUrl: "https://relay.example.com/v1",
      modelsDev,
    });
    expect(metadata).toMatchObject({
      contextWindow: 204800,
      reasoning: true,
      inputModalities: ["text"],
      sources: ["models-dev"],
    });
    expect(metadata?.cost).toBeUndefined();
    expect(metadata?.reasoningEfforts).toBeUndefined();
  });

  it("keeps the price when the provider itself is recognized", () => {
    expect(
      resolveModelMetadata("glm-5", {
        baseUrl: "https://open.bigmodel.cn/api/paas/v4",
        modelsDev,
      })?.cost,
    ).toEqual({ input: 1, output: 3.2 });
  });

  it("gives up when the same name points at different vendors", () => {
    expect(
      resolveModelMetadata("kimi-k9", {
        baseUrl: "https://relay.example.com/v1",
        modelsDev,
      }),
    ).toBeNull();
  });

  it("lets the matching preset row win field by field", () => {
    const presets = [
      preset("https://api.z.ai/api/coding/paas/v4", {
        "glm-5": { contextWindow: 500000 },
      }),
      preset("https://relay.example.com/v1", {
        "glm-5": { contextWindow: 1 },
      }),
    ];
    expect(
      resolveModelMetadata("glm-5", {
        baseUrl: "https://api.z.ai/api/coding/paas/v4",
        presets,
        modelsDev,
      }),
    ).toMatchObject({
      contextWindow: 500000,
      maxOutputTokens: 98304,
      sources: ["preset", "models-dev"],
    });
  });

  it("does not take another plan's preset on the same host", () => {
    expect(
      resolveModelMetadata("glm-5", {
        baseUrl: "https://api.z.ai/api/paas/v4",
        presets: [
          preset("https://api.z.ai/api/coding/paas/v4", {
            "glm-5": { contextWindow: 500000 },
          }),
        ],
        modelsDev,
      }),
    ).toMatchObject({ contextWindow: 200000, sources: ["models-dev"] });
  });

  it("accepts the same endpoint written with or without a version suffix", () => {
    expect(
      resolveModelMetadata("glm-5.3", {
        baseUrl: "https://opencode.ai/zen/go",
        modelsDev,
      })?.contextWindow,
    ).toBe(1000000);
  });

  it("does not treat a parent path as the plans nested under it", () => {
    expect(
      resolveModelMetadata("glm-5.3", {
        baseUrl: "https://opencode.ai/zen",
        presets: [
          preset("https://opencode.ai/zen/go/v1", {
            "glm-5.3": { contextWindow: 1 },
          }),
        ],
        modelsDev,
      }),
    ).toBeNull();
  });

  it("ignores a pasted request path after the base address", () => {
    expect(
      resolveModelMetadata("glm-5", {
        baseUrl: "https://api.z.ai/api/coding/paas/v4/chat/completions",
        modelsDev,
      })?.contextWindow,
    ).toBe(202752);
  });

  it("fills fields the provider entry leaves out from the vendor entry", () => {
    const metadata = resolveModelMetadata("glm-5", {
      baseUrl: "https://api.z.ai/api/paas/v4",
      modelsDev,
    });
    expect(metadata).toMatchObject({
      contextWindow: 200000,
      maxOutputTokens: 128000,
      reasoning: true,
      inputModalities: ["text"],
    });
    expect(metadata?.cost).toBeUndefined();
  });

  it("uses presets alone when models.dev is unavailable", () => {
    expect(
      resolveModelMetadata("glm-5", {
        baseUrl: "https://api.z.ai/api/coding/paas/v4",
        presets: [
          preset("https://api.z.ai/api/coding/paas/v4", {
            "glm-5": { contextWindow: 500000 },
          }),
        ],
      }),
    ).toEqual({ contextWindow: 500000, sources: ["preset"] });
    expect(
      resolveModelMetadata("glm-5", { baseUrl: "https://api.z.ai/v1" }),
    ).toBeNull();
  });
});
