import { describe, expect, it } from "vitest";
import {
  endpointsMatch,
  metadataFromGateway,
  resolveModelMetadata,
} from "./modelMetadata";

describe("endpointsMatch", () => {
  it("忽略版本段和请求路径", () => {
    expect(
      endpointsMatch("https://api.example.com/v1", "https://api.example.com"),
    ).toBe(true);
    expect(
      endpointsMatch(
        "https://api.example.com/v1/chat/completions",
        "https://api.example.com",
      ),
    ).toBe(true);
    expect(
      endpointsMatch("https://api.example.com/v1", "https://api.z.ai/v1"),
    ).toBe(false);
  });
});

describe("metadataFromGateway", () => {
  it("读取网关的 info.meta 信封", () => {
    expect(
      metadataFromGateway({
        meta: {
          capabilities: { vision: true },
          models_dev: {
            name: "DeepSeek V4.1 Flash",
            family: "deepseek-flash",
            release_date: "2026-09-10",
            status: "active",
            attachment: true,
            temperature: true,
            tool_call: true,
            interleaved: { field: "reasoning_content" },
            limit: { context: 1_050_000, input: 900_000, output: 128_000 },
            cost: {
              input: 0.1,
              output: 0.5,
              cache_read: 0.01,
              cache_write: 0.125,
              context_over_200k: {
                input: 0.2,
                output: 0.8,
                cache_read: 0.004,
              },
            },
            modalities: { input: ["text", "image"], output: ["text"] },
            reasoning: true,
            reasoning_options: [
              { type: "effort", values: ["low", "high"] },
              { type: "budget", values: ["ignored"] },
            ],
          },
        },
      }),
    ).toEqual({
      displayName: "DeepSeek V4.1 Flash",
      contextWindow: 1_050_000,
      maxInputTokens: 900_000,
      maxOutputTokens: 128_000,
      family: "deepseek-flash",
      releaseDate: "2026-09-10",
      attachment: true,
      temperature: true,
      toolCall: true,
      interleaved: { field: "reasoning_content" },
      status: "active",
      reasoning: true,
      reasoningEfforts: ["low", "high"],
      inputModalities: ["text", "image"],
      outputModalities: ["text"],
      cost: {
        input: 0.1,
        output: 0.5,
        cacheRead: 0.01,
        cacheWrite: 0.125,
      },
      costOver200k: {
        input: 0.2,
        output: 0.8,
        cacheRead: 0.004,
      },
    });
  });

  it("接受直接放在条目上的 meta，并用 vision 兜底模态", () => {
    expect(
      metadataFromGateway({ n_ctx: 8192, max_tokens: 2048 }),
    ).toMatchObject({ contextWindow: 8192, maxOutputTokens: 2048 });
    expect(
      metadataFromGateway({ capabilities: { vision: false } }),
    ).toBeUndefined();
    expect(
      metadataFromGateway({
        capabilities: { vision: false, vision_source: "models.dev" },
      }),
    ).toEqual({ inputModalities: ["text"] });
    expect(metadataFromGateway({ capabilities: { vision: true } })).toEqual({
      inputModalities: ["text", "image"],
    });
  });

  it("没有可用字段时返回 undefined", () => {
    expect(metadataFromGateway(undefined)).toBeUndefined();
    expect(metadataFromGateway(null)).toBeUndefined();
    expect(metadataFromGateway({})).toBeUndefined();
    expect(metadataFromGateway({ meta: {} })).toBeUndefined();
    expect(metadataFromGateway("not-an-object")).toBeUndefined();
  });
});

describe("resolveModelMetadata 网关来源", () => {
  it("网关参数优先，且 GPT 窗口不被原厂兜底封顶", () => {
    expect(
      resolveModelMetadata("gpt-6-luna", {
        baseUrl: "http://127.0.0.1:8787/v1",
        gatewayMetadata: { contextWindow: 1_050_000, reasoning: true },
      }),
    ).toEqual({
      contextWindow: 1_050_000,
      reasoning: true,
      sources: ["gateway"],
    });
  });

  it("网关缺的字段仍由预设或 models.dev 补", () => {
    const metadata = resolveModelMetadata("glm-5.3-flash", {
      baseUrl: "https://example.com/v1",
      presets: [
        {
          baseUrl: "https://example.com/v1",
          models: new Map([
            ["glm-5.3-flash", { contextWindow: 200_000, reasoning: true }],
          ]),
        },
      ],
      gatewayMetadata: { maxOutputTokens: 131_072 },
    });
    expect(metadata).toEqual({
      contextWindow: 200_000,
      maxOutputTokens: 131_072,
      reasoning: true,
      sources: ["gateway", "preset"],
    });
  });
});
