import { describe, expect, it } from "vitest";
import { parse as parseToml } from "smol-toml";
import {
  buildGrokBuildConfig,
  extractGrokBuildBaseUrl,
  parseGrokBuildConfig,
} from "./grokBuildConfig";

describe("Grok Build config", () => {
  it("builds the expected provider TOML", () => {
    const config = buildGrokBuildConfig({
      model: "grok-4.5",
      baseUrl: "https://relay.example.com/v1",
      name: 'Relay "A"',
      apiKey: "secret",
      apiBackend: "responses",
      contextWindow: 500000,
    });
    const parsed = parseToml(config) as any;

    expect(parsed.models.default).toBe("grok-4.5");
    expect(parsed.model["grok-4.5"]).toEqual({
      model: "grok-4.5",
      base_url: "https://relay.example.com/v1",
      name: 'Relay "A"',
      api_key: "secret",
      api_backend: "responses",
      context_window: 500000,
    });
    expect(config).toContain('[model."grok-4.5"]');
  });

  it("reads values back from a generated config", () => {
    const config = buildGrokBuildConfig({
      model: "custom-model",
      upstreamModel: "upstream-model",
      baseUrl: "https://api.example.com",
      name: "Custom",
      apiKey: "key",
      apiBackend: "responses",
      contextWindow: 320000,
    });

    expect(parseGrokBuildConfig(config)).toEqual({
      model: "custom-model",
      upstreamModel: "upstream-model",
      baseUrl: "https://api.example.com",
      name: "Custom",
      apiKey: "key",
      apiBackend: "responses",
      contextWindow: 320000,
    });
    expect(extractGrokBuildBaseUrl(config)).toBe("https://api.example.com");
  });
});
