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
      extraModels: [],
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

  it("writes every model on one gateway and keeps reasoning menus", () => {
    const config = updateGrokBuildConfig(
      `[ui]
max_thoughts_width = 120

[models]
default = "grok-4.7"

[model."grok-4.7"]
model = "grok-4.7"
base_url = "https://old.example/v1"
name = "Old"
api_key = "old-key"
api_backend = "responses"
context_window = 500000
description = "keep me"
`,
      {
        model: "grok-4.7",
        upstreamModel: "grok-4.7",
        baseUrl: "https://gateway.example/v1",
        name: "Person 4.7",
        apiKey: "secret",
        apiBackend: "responses",
        contextWindow: 500000,
        reasoningLevels: ["low", "high", "xhigh"],
        defaultReasoningLevel: "xhigh",
        extraModels: [
          {
            model: "grok-4.6",
            name: "Person 4.6",
            contextWindow: 500000,
            reasoningLevels: ["low", "medium", "high", "xhigh"],
            defaultReasoningLevel: "high",
          },
        ],
      },
    );
    const parsed = parseToml(config) as any;

    expect(parsed.ui.max_thoughts_width).toBe(120);
    expect(parsed.models.default).toBe("grok-4.7");
    expect(parsed.models.default_reasoning_effort).toBe("xhigh");
    expect(parsed.model["grok-4.7"].description).toBe("keep me");
    expect(parsed.model["grok-4.7"].base_url).toBe(
      "https://gateway.example/v1",
    );
    expect(parsed.model["grok-4.7"].reasoning_efforts).toEqual([
      { value: "low", label: "Low Effort" },
      { value: "high", label: "High Effort" },
      { value: "xhigh", label: "Extra High Effort", default: true },
    ]);
    expect(parsed.model["grok-4.6"]).toMatchObject({
      model: "grok-4.6",
      name: "Person 4.6",
      base_url: "https://gateway.example/v1",
      api_key: "secret",
      api_backend: "responses",
    });
    expect(
      parsed.model["grok-4.6"].reasoning_efforts.find(
        (effort: { value: string }) => effort.value === "high",
      ).default,
    ).toBe(true);

    const readBack = parseGrokBuildConfig(config);
    expect(readBack.model).toBe("grok-4.7");
    expect(readBack.reasoningLevels).toEqual(["low", "high", "xhigh"]);
    expect(readBack.defaultReasoningLevel).toBe("xhigh");
    expect(readBack.extraModels).toEqual([
      {
        profile: "grok-4.6",
        model: "grok-4.6",
        name: "Person 4.6",
        contextWindow: 500000,
        reasoningLevels: ["low", "medium", "high", "xhigh"],
        defaultReasoningLevel: "high",
      },
    ]);
  });

  it("drops a removed extra model without touching an unmanaged catalog", () => {
    const original = updateGrokBuildConfig(undefined, {
      model: "grok-4.7",
      baseUrl: "https://gateway.example/v1",
      name: "Person",
      apiKey: "secret",
      apiBackend: "responses",
      contextWindow: 500000,
      extraModels: [{ model: "grok-4.6", name: "Person 4.6" }],
    });

    const removed = updateGrokBuildConfig(original, {
      ...parseGrokBuildConfig(original),
      extraModels: [],
    });
    const parsed = parseToml(removed) as any;
    expect(Object.keys(parsed.model)).toEqual(["grok-4.7"]);

    const unmanaged = updateGrokBuildConfig(original, {
      ...parseGrokBuildConfig(original),
      baseUrl: "https://gateway.example/v2",
      extraModels: undefined,
    });
    const unmanagedParsed = parseToml(unmanaged) as any;
    expect(unmanagedParsed.model["grok-4.7"].base_url).toBe(
      "https://gateway.example/v2",
    );
    expect(unmanagedParsed.model["grok-4.6"].base_url).toBe(
      "https://gateway.example/v1",
    );
  });
});
