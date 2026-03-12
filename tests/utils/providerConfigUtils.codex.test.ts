import { describe, expect, it } from "vitest";
import {
  extractCodexBaseUrl,
  extractCodexModelName,
  setCodexBaseUrl,
  setCodexModelName,
} from "@/utils/providerConfigUtils";

describe("Codex TOML utils", () => {
  it("removes base_url line when set to empty", () => {
    const input = [
      'model_provider = "openai"',
      'base_url = "https://api.example.com/v1"',
      'model = "gpt-5-codex"',
      "",
    ].join("\n");

    const output = setCodexBaseUrl(input, "");

    expect(output).not.toMatch(/^\s*base_url\s*=/m);
    expect(extractCodexBaseUrl(output)).toBeUndefined();
    expect(extractCodexModelName(output)).toBe("gpt-5-codex");
  });

  it("removes model line when set to empty", () => {
    const input = [
      'model_provider = "openai"',
      'base_url = "https://api.example.com/v1"',
      'model = "gpt-5-codex"',
      "",
    ].join("\n");

    const output = setCodexModelName(input, "");

    expect(output).not.toMatch(/^\s*model\s*=/m);
    expect(extractCodexModelName(output)).toBeUndefined();
    expect(extractCodexBaseUrl(output)).toBe("https://api.example.com/v1");
  });

  it("updates existing values when non-empty", () => {
    const input = [
      'model_provider = "openai"',
      "base_url = 'https://old.example/v1'",
      'model = "old-model"',
      "",
    ].join("\n");

    const output1 = setCodexBaseUrl(input, " https://new.example/v1 \n");
    expect(extractCodexBaseUrl(output1)).toBe("https://new.example/v1");

    const output2 = setCodexModelName(output1, " new-model \n");
    expect(extractCodexModelName(output2)).toBe("new-model");
  });

  it("writes base_url into the active provider section when other sections follow", () => {
    const input = [
      'model_provider = "custom"',
      'model = "gpt-5.4"',
      "",
      "[model_providers.custom]",
      'name = "custom"',
      'wire_api = "responses"',
      "",
      "[profiles.default]",
      'approval_policy = "never"',
      "",
    ].join("\n");

    const output = setCodexBaseUrl(input, "https://api.example.com/v1");

    expect(output).toContain(
      '[model_providers.custom]\nname = "custom"\nwire_api = "responses"\nbase_url = "https://api.example.com/v1"',
    );
    expect(output).not.toContain(
      '[profiles.default]\napproval_policy = "never"\nbase_url = "https://api.example.com/v1"',
    );
    expect(extractCodexBaseUrl(output)).toBe("https://api.example.com/v1");
  });

  it("moves a misplaced base_url back into the active provider section", () => {
    const input = [
      'model_provider = "custom"',
      'model = "gpt-5.4"',
      "",
      "[model_providers.custom]",
      'name = "custom"',
      'wire_api = "responses"',
      "",
      "[profiles.default]",
      'approval_policy = "never"',
      'base_url = "https://wrong.example/v1"',
      "",
    ].join("\n");

    const output = setCodexBaseUrl(input, "https://fixed.example/v1");

    expect(output).toContain(
      '[model_providers.custom]\nname = "custom"\nwire_api = "responses"\nbase_url = "https://fixed.example/v1"',
    );
    expect(output).not.toContain("https://wrong.example/v1");
    expect(output.match(/base_url\s*=/g)).toHaveLength(1);
    expect(extractCodexBaseUrl(output)).toBe("https://fixed.example/v1");
  });

  it("prefers the active provider section when extracting base_url", () => {
    const input = [
      'model_provider = "custom"',
      'model = "gpt-5.4"',
      "",
      "[model_providers.custom]",
      'name = "custom"',
      'base_url = "https://provider.example/v1"',
      'wire_api = "responses"',
      "",
      "[profiles.default]",
      'approval_policy = "never"',
      'base_url = "https://profile.example/v1"',
      "",
    ].join("\n");

    expect(extractCodexBaseUrl(input)).toBe("https://provider.example/v1");
  });

  it("falls back to a misplaced base_url when the provider section has none", () => {
    const input = [
      'model_provider = "custom"',
      'model = "gpt-5.4"',
      "",
      "[model_providers.custom]",
      'name = "custom"',
      'wire_api = "responses"',
      "",
      "[profiles.default]",
      'approval_policy = "never"',
      'base_url = "https://misplaced.example/v1"',
      "",
    ].join("\n");

    expect(extractCodexBaseUrl(input)).toBe("https://misplaced.example/v1");
  });
});
