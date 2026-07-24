import { describe, expect, it } from "vitest";
import { parse as parseToml } from "smol-toml";
import {
  buildKimiCodeConfig,
  buildSettingsConfig,
  parseKimiCodeConfig,
  validateKimiCodeConfig,
  type KimiProviderFormState,
} from "./kimiCodeConfig";

const state: KimiProviderFormState = {
  providerId: "my-openai",
  providerType: "openai",
  baseUrl: "https://example.invalid/v1",
  apiKey: "test-key",
  customHeaders: { "X-Test": "preserved" },
  models: [
    {
      alias: "my-openai/model-a",
      model: "model-a",
      maxContextSize: 128000,
      capabilities: ["tool_use"],
      displayName: "Model A",
    },
    {
      alias: "my-openai/model-b",
      model: "model-b",
      maxContextSize: 200000,
      maxOutputSize: 8192,
    },
  ],
  selectedModel: "my-openai/model-b",
};

describe("kimiCodeConfig", () => {
  it("round-trips a scoped provider fragment", () => {
    const config = buildKimiCodeConfig(state);
    expect(validateKimiCodeConfig(config)).toBeNull();
    expect(parseKimiCodeConfig(config)).toEqual(state);

    const parsed = parseToml(config) as Record<string, unknown>;
    expect(parsed).not.toHaveProperty("default_model");
    expect(parsed).toHaveProperty("selected_model", "my-openai/model-b");
  });

  it.each([
    "default_model",
    "default_permission_mode",
    "thinking",
    "loop_control",
    "background",
    "image",
    "services",
    "permission",
    "hooks",
  ])("rejects shared top-level field %s", (field) => {
    const base = buildKimiCodeConfig(state);
    const value = [
      "thinking",
      "loop_control",
      "background",
      "image",
      "services",
      "permission",
    ].includes(field)
      ? `\n[${field}]\nenabled = true\n`
      : field === "hooks"
        ? '\n[[hooks]]\nevent = "PreToolUse"\ncommand = "true"\n'
        : `\n${field} = "forbidden"\n`;
    expect(validateKimiCodeConfig(value + base)).toBe("forbidden");
  });

  it("sanitizes a fallback provider id and stores matching metadata", () => {
    const fallback = parseKimiCodeConfig(undefined, "My Custom Provider!");
    expect(fallback.providerId).toBe("my-custom-provider");
    expect(fallback.selectedModel).toBe("my-custom-provider/default");

    expect(buildSettingsConfig(fallback)).toMatchObject({
      provider_id: "my-custom-provider",
      selected_model: "my-custom-provider/default",
    });
  });

  it("rejects malformed or unscoped fragments", () => {
    expect(validateKimiCodeConfig("not = [valid")).toBe("toml");
    expect(validateKimiCodeConfig('[providers.a]\ntype = "openai"\n')).toBe(
      "models",
    );
  });
});
