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
    expect(parsed.models.session_summary).toBe("grok-4.5");
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
    expect((parseToml(config) as any).models.session_summary).toBe(
      "custom-model",
    );
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
    expect(parsed.models.session_summary).toBe("new-profile");
    expect(parsed.model["new-profile"].model).toBe("grok-upstream");
    expect(parsed.model).not.toHaveProperty("old-profile");
  });

  it("sets the summary profile when editing a legacy config without one", () => {
    const original = `[models]
default = "custom-profile"

[model."custom-profile"]
model = "upstream-model"
base_url = "https://relay.example.com/v1"
name = "Relay"
api_key = "test-key"
api_backend = "responses"
context_window = 500000
`;
    const updated = updateGrokBuildConfig(original, {
      ...parseGrokBuildConfig(original),
      name: "Updated Relay",
    });
    const parsed = parseToml(updated) as any;

    expect(parsed.models.session_summary).toBe("custom-profile");
    expect(parsed.model[parsed.models.session_summary].model).toBe(
      "upstream-model",
    );
  });

  it("preserves a separately configured summary model when renaming the default", () => {
    const original = `[models]
default = "old-profile"
session_summary = "title-profile"
web_search = "search-profile"

[model."old-profile"]
model = "upstream-model"
base_url = "https://relay.example.com/v1"
name = "Relay"
api_key = "test-key"
api_backend = "responses"
context_window = 500000

[model."title-profile"]
model = "small-model"
base_url = "https://title.example.com/v1"
env_key = "TITLE_API_KEY"
`;
    const updated = updateGrokBuildConfig(original, {
      ...parseGrokBuildConfig(original),
      model: "new-profile",
    });
    const parsed = parseToml(updated) as any;

    expect(parsed.models.default).toBe("new-profile");
    expect(parsed.models.session_summary).toBe("title-profile");
    expect(parsed.models.web_search).toBe("search-profile");
    expect(parsed.model["title-profile"]).toEqual(
      (parseToml(original) as any).model["title-profile"],
    );
    expect(parsed.model).not.toHaveProperty("old-profile");
  });
});
