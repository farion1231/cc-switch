import { describe, expect, it } from "vitest";
import {
  resolveKnownBaseUrlForApiFormat,
  resolveKnownOpencodeBaseUrl,
} from "@/components/providers/forms/helpers/apiFormatBaseUrl";

describe("resolveKnownBaseUrlForApiFormat", () => {
  it.each([
    [
      "https://api.minimax.cn/v1",
      "anthropic",
      "https://api.minimax.cn/anthropic",
    ],
    [
      "https://api.minimax.io/v1",
      "anthropic",
      "https://api.minimax.io/anthropic",
    ],
    [
      "https://api.minimaxi.com/v1",
      "anthropic",
      "https://api.minimaxi.com/anthropic",
    ],
    [
      "https://api.minimax.cn/anthropic",
      "openai_chat",
      "https://api.minimax.cn/v1",
    ],
    [
      "https://api.minimax.io/anthropic",
      "openai_responses",
      "https://api.minimax.io/v1",
    ],
    [
      "https://api.minimaxi.com/anthropic",
      "openai_chat",
      "https://api.minimaxi.com/v1",
    ],
  ] as const)("maps %s for %s", (baseUrl, apiFormat, expected) => {
    expect(resolveKnownBaseUrlForApiFormat(baseUrl, apiFormat)).toBe(expected);
  });

  it("matches a known template after a trailing slash", () => {
    expect(
      resolveKnownBaseUrlForApiFormat(
        "https://api.minimax.cn/v1///",
        "anthropic",
      ),
    ).toBe("https://api.minimax.cn/anthropic");
  });

  it.each([
    ["https://relay.example.com/v1", "anthropic"],
    ["https://api.minimax.cn/v1/models", "anthropic"],
    ["https://api.minimax.cn/v1?tenant=example", "anthropic"],
    ["https://api.minimax.cn/v1", "gemini_native"],
  ] as const)("does not rewrite %s for %s", (baseUrl, apiFormat) => {
    expect(resolveKnownBaseUrlForApiFormat(baseUrl, apiFormat)).toBeNull();
  });

  it("does not report a change when the base URL already matches", () => {
    expect(
      resolveKnownBaseUrlForApiFormat(
        "https://api.minimax.cn/anthropic",
        "anthropic",
      ),
    ).toBeNull();
  });
});

describe("resolveKnownOpencodeBaseUrl", () => {
  it("maps supported OpenCode npm packages to their protocol path", () => {
    expect(
      resolveKnownOpencodeBaseUrl(
        "https://api.minimax.cn/v1",
        "@ai-sdk/anthropic",
      ),
    ).toBe("https://api.minimax.cn/anthropic");
    expect(
      resolveKnownOpencodeBaseUrl(
        "https://api.minimax.io/anthropic",
        "@ai-sdk/openai-compatible",
      ),
    ).toBe("https://api.minimax.io/v1");
    expect(
      resolveKnownOpencodeBaseUrl(
        "https://api.minimax.io/anthropic",
        "@ai-sdk/openai",
      ),
    ).toBe("https://api.minimax.io/v1");
  });

  it("does not rewrite a custom URL", () => {
    expect(
      resolveKnownOpencodeBaseUrl(
        "https://relay.example.com/v1",
        "@ai-sdk/anthropic",
      ),
    ).toBeNull();
  });
});
