import { describe, expect, it } from "vitest";
import { extractProviderBaseUrl } from "@/utils/providerConfigUtils";

describe("extractProviderBaseUrl", () => {
  it.each([
    [
      "claude / claude-desktop",
      { env: { ANTHROPIC_BASE_URL: "https://claude.example.com" } },
      "https://claude.example.com",
    ],
    [
      "gemini",
      { env: { GOOGLE_GEMINI_BASE_URL: "https://gemini.example.com" } },
      "https://gemini.example.com",
    ],
    [
      "codex / grokbuild",
      {
        auth: {},
        config: [
          'model_provider = "custom"',
          "",
          "[model_providers.custom]",
          'base_url = "https://codex.example.com/v1"',
        ].join("\n"),
      },
      "https://codex.example.com/v1",
    ],
    [
      "opencode / mcode",
      { options: { baseURL: "https://opencode.example.com/v1" } },
      "https://opencode.example.com/v1",
    ],
    [
      "openclaw / pi",
      { baseUrl: "https://openclaw.example.com/v1" },
      "https://openclaw.example.com/v1",
    ],
    [
      "hermes",
      { base_url: "https://hermes.example.com/v1" },
      "https://hermes.example.com/v1",
    ],
  ])("reads the %s shape", (_app, settingsConfig, expected) => {
    expect(extractProviderBaseUrl(settingsConfig)).toBe(expected);
  });

  it("returns undefined for official providers without an address", () => {
    expect(extractProviderBaseUrl({ env: {} })).toBeUndefined();
    expect(extractProviderBaseUrl({ auth: {}, config: "" })).toBeUndefined();
    expect(extractProviderBaseUrl(undefined)).toBeUndefined();
  });
});
