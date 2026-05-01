import { describe, expect, it } from "vitest";
import {
  shouldPersistFullUrl,
  supportsFullUrlMode,
} from "@/components/providers/forms/helpers/fullUrlSupport";

describe("providerFullUrlSupport", () => {
  it("keeps full URL mode enabled for Claude and Codex non-official providers", () => {
    expect(
      supportsFullUrlMode({
        appId: "claude",
        category: "third_party",
      }),
    ).toBe(true);

    expect(
      supportsFullUrlMode({
        appId: "codex",
        category: "custom",
      }),
    ).toBe(true);
  });

  it("only enables full URL mode for supported OpenCode npm packages", () => {
    expect(
      supportsFullUrlMode({
        appId: "opencode",
        category: "third_party",
        opencodeNpm: "@ai-sdk/openai-compatible",
      }),
    ).toBe(true);

    expect(
      supportsFullUrlMode({
        appId: "opencode",
        category: "aggregator",
        opencodeNpm: "@ai-sdk/openai",
      }),
    ).toBe(true);

    expect(
      supportsFullUrlMode({
        appId: "opencode",
        category: "custom",
        opencodeNpm: "@ai-sdk/anthropic",
      }),
    ).toBe(true);

    expect(
      supportsFullUrlMode({
        appId: "opencode",
        category: "custom",
        opencodeNpm: "@ai-sdk/google",
      }),
    ).toBe(false);

    expect(
      supportsFullUrlMode({
        appId: "opencode",
        category: "omo",
        opencodeNpm: "@ai-sdk/openai-compatible",
      }),
    ).toBe(false);
  });

  it("only enables full URL mode for supported OpenClaw protocols", () => {
    expect(
      supportsFullUrlMode({
        appId: "openclaw",
        category: "third_party",
        openclawApi: "openai-completions",
      }),
    ).toBe(true);

    expect(
      supportsFullUrlMode({
        appId: "openclaw",
        category: "custom",
        openclawApi: "openai-responses",
      }),
    ).toBe(true);

    expect(
      supportsFullUrlMode({
        appId: "openclaw",
        category: "aggregator",
        openclawApi: "anthropic-messages",
      }),
    ).toBe(true);

    expect(
      supportsFullUrlMode({
        appId: "openclaw",
        category: "custom",
        openclawApi: "google-generative-ai",
      }),
    ).toBe(false);

    expect(
      supportsFullUrlMode({
        appId: "openclaw",
        category: "cloud_provider",
        openclawApi: "bedrock-converse-stream",
      }),
    ).toBe(false);
  });

  it("only persists full URL when the combination is supported and enabled", () => {
    expect(
      shouldPersistFullUrl({
        appId: "claude",
        category: "third_party",
        isFullUrl: true,
      }),
    ).toBe(true);

    expect(
      shouldPersistFullUrl({
        appId: "opencode",
        category: "custom",
        opencodeNpm: "@ai-sdk/openai-compatible",
        isFullUrl: true,
      }),
    ).toBe(true);

    expect(
      shouldPersistFullUrl({
        appId: "openclaw",
        category: "custom",
        openclawApi: "openai-responses",
        isFullUrl: true,
      }),
    ).toBe(true);

    expect(
      shouldPersistFullUrl({
        appId: "opencode",
        category: "custom",
        opencodeNpm: "@ai-sdk/google",
        isFullUrl: true,
      }),
    ).toBe(false);

    expect(
      shouldPersistFullUrl({
        appId: "openclaw",
        category: "cloud_provider",
        openclawApi: "bedrock-converse-stream",
        isFullUrl: true,
      }),
    ).toBe(false);

    expect(
      shouldPersistFullUrl({
        appId: "codex",
        category: "custom",
        isFullUrl: false,
      }),
    ).toBe(false);

    expect(
      shouldPersistFullUrl({
        appId: "claude",
        category: "official",
        isFullUrl: true,
      }),
    ).toBe(false);
  });
});
