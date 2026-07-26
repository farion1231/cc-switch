import { describe, expect, it } from "vitest";
import {
  formatTokenCount,
  inferContextWindowTokens,
  inferProviderGroup,
  normalizeCursorEndpoint,
  resolveCursorEndpointGroup,
  resolveCursorModelMetadata,
} from "./cursorModelMetadata";

describe("cursorModelMetadata", () => {
  it("优先使用提供商返回的上下文大小", () => {
    expect(
      resolveCursorModelMetadata(
        {
          id: "gpt-5-custom",
          ownedBy: "Relay Vendor",
          contextWindowTokens: 123_456,
        },
        "https://relay.example.com/v1",
        "openai",
      ),
    ).toEqual({
      providerGroup: "relay.example.com",
      contextWindowTokens: 123_456,
      contextWindowSource: "provider",
    });
  });

  it("在接口缺少元数据时根据模型族推断", () => {
    expect(inferContextWindowTokens("claude-sonnet-4-6")).toBe(200_000);
    expect(inferContextWindowTokens("vendor/qwen3-coder")).toBe(262_144);
    expect(inferContextWindowTokens("unknown-model")).toBe(0);
  });

  it("根据 Endpoint 而不是 ownedBy 确定提供商名称", () => {
    expect(
      inferProviderGroup(
        { ownedBy: "OpenAI" },
        "https://openrouter.ai/api/v1",
        "openai",
      ),
    ).toBe("OpenRouter");
    expect(
      inferProviderGroup(
        { ownedBy: null },
        "https://api.deepseek.com/v1",
        "openai",
      ),
    ).toBe("DeepSeek");
    expect(inferProviderGroup({ ownedBy: null }, "invalid", "anthropic")).toBe(
      "Anthropic Compatible",
    );
  });

  it("用规范化 Base URL 作为分组身份", () => {
    expect(normalizeCursorEndpoint("https://relay.example.com/v1/")).toBe(
      "https://relay.example.com/v1",
    );
    expect(
      resolveCursorEndpointGroup(
        "https://relay.example.com/v1/",
        "主线路",
        "openai",
      ),
    ).toEqual({
      key: "openai:https://relay.example.com/v1",
      label: "主线路",
      baseUrl: "https://relay.example.com/v1/",
    });
    expect(
      resolveCursorEndpointGroup(
        "https://relay.example.com/v1",
        "主线路",
        "anthropic",
      ).key,
    ).not.toBe(
      resolveCursorEndpointGroup(
        "https://relay.example.com/v1",
        "主线路",
        "openai",
      ).key,
    );
    expect(
      normalizeCursorEndpoint("https://relay.example.com/anthropic"),
    ).not.toBe(normalizeCursorEndpoint("https://relay.example.com/v1"));
  });

  it("以紧凑格式展示 token 数", () => {
    expect(formatTokenCount(200_000)).toBe("200K");
    expect(formatTokenCount(1_000_000)).toBe("1M");
  });
});
