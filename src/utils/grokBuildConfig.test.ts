import { describe, expect, it } from "vitest";
import { parse as parseToml } from "smol-toml";
import {
  buildGrokBuildConfig,
  extractGrokBuildBaseUrl,
  parseGrokBuildConfig,
  updateGrokBuildConfig,
  validateGrokBuildConfig,
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
      envKey: "",
      apiBackend: "responses",
      contextWindow: 320000,
    });

    expect(parseGrokBuildConfig(config)).toEqual({
      model: "custom-model",
      upstreamModel: "upstream-model",
      baseUrl: "https://api.example.com",
      name: "Custom",
      apiKey: "key",
      envKey: "",
      apiBackend: "responses",
      contextWindow: 320000,
    });
    expect(extractGrokBuildBaseUrl(config)).toBe("https://api.example.com");
  });

  it("accepts env_key credentials without adding an empty api_key", () => {
    const config = `[models]
default = "env-profile"

[model."env-profile"]
model = "grok-4.5"
base_url = "https://api.example.com/v1"
name = "Env Relay"
env_key = "XAI_API_KEY"
api_backend = "responses"
context_window = 500000
`;

    expect(validateGrokBuildConfig(config)).toBeNull();
    expect(parseGrokBuildConfig(config).envKey).toBe("XAI_API_KEY");

    const updated = updateGrokBuildConfig(config, {
      ...parseGrokBuildConfig(config),
      baseUrl: "https://updated.example.com/v1",
    });
    const parsed = parseToml(updated) as any;
    expect(parsed.model["env-profile"].env_key).toBe("XAI_API_KEY");
    expect(parsed.model["env-profile"]).not.toHaveProperty("api_key");
  });

  it("reports malformed, incomplete, and invalid-window configs", () => {
    expect(validateGrokBuildConfig("")).toBe("config.toml must not be empty");
    expect(validateGrokBuildConfig("[models")).not.toBeNull();
    expect(validateGrokBuildConfig('[models]\ndefault = "missing"\n')).toBe(
      "Missing [models] default model table",
    );

    const missingCredentials = buildGrokBuildConfig({
      model: "grok-4.5",
      baseUrl: "https://api.example.com/v1",
      name: "Relay",
      apiKey: "",
      apiBackend: "responses",
      contextWindow: 500000,
    });
    expect(validateGrokBuildConfig(missingCredentials)).toBe(
      "Missing api_key or env_key",
    );

    const invalidWindow = missingCredentials.replace(
      "context_window = 500000",
      "context_window = 0",
    );
    expect(validateGrokBuildConfig(invalidWindow)).toBe(
      "Missing api_key or env_key",
    );
    expect(
      validateGrokBuildConfig(
        invalidWindow.replace(
          'name = "Relay"',
          'name = "Relay"\napi_key = "secret"',
        ),
      ),
    ).toBe("context_window must be a positive integer");
  });

  it("renames the selected profile without leaving the old table behind", () => {
    const original = buildGrokBuildConfig({
      model: "old-profile",
      upstreamModel: "grok-upstream",
      baseUrl: "https://api.example.com/v1",
      name: "Relay",
      apiKey: "secret",
      apiBackend: "responses",
      contextWindow: 500000,
    });

    const renamed = updateGrokBuildConfig(original, {
      ...parseGrokBuildConfig(original),
      model: "new-profile",
    });
    const parsed = parseToml(renamed) as any;

    expect(parsed.models.default).toBe("new-profile");
    expect(parsed.model["new-profile"].model).toBe("grok-upstream");
    expect(parsed.model).not.toHaveProperty("old-profile");
  });
});
