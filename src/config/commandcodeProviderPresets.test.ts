import { describe, expect, it } from "vitest";
import {
  COMMANDCODE_API_FORMATS,
  commandcodeProviderPresets,
} from "./commandcodeProviderPresets";
import { piProviderPresets } from "./piProviderPresets";

const COMMANDCODE_PROVIDER_ID = /^[a-z0-9][a-z0-9_.-]{0,63}$/;
const RESERVED_PROVIDER_IDS = new Set([
  "anthropic",
  "github-copilot",
  "codex",
  "apikey",
  "userid",
  "username",
  "keyname",
  "authenticatedat",
]);

const preset = (name: string) =>
  commandcodeProviderPresets.find((candidate) => candidate.name === name);

describe("commandcodeProviderPresets", () => {
  it("keeps every preset inside the ids and API kinds Command Code accepts", () => {
    expect(commandcodeProviderPresets.length).toBeGreaterThan(0);
    for (const entry of commandcodeProviderPresets) {
      expect(COMMANDCODE_API_FORMATS, entry.name).toContain(
        entry.settingsConfig.api,
      );
      expect(entry.providerKey, entry.name).toMatch(COMMANDCODE_PROVIDER_ID);
      expect(
        RESERVED_PROVIDER_IDS.has(entry.providerKey.toLowerCase()),
        entry.name,
      ).toBe(false);
    }
  });

  it("gives every preset an http(s) endpoint and at least one model", () => {
    for (const entry of commandcodeProviderPresets) {
      expect(entry.settingsConfig.baseURL, entry.name).toMatch(/^https?:\/\//);
      expect(
        Object.keys(entry.settingsConfig.models).length,
        entry.name,
      ).toBeGreaterThan(0);
    }
  });

  it("writes models in the shape Command Code emits, not Pi's", () => {
    for (const entry of commandcodeProviderPresets) {
      for (const [id, model] of Object.entries(entry.settingsConfig.models)) {
        expect(model.name, `${entry.name}/${id}`).toBeTruthy();
        expect(model).not.toHaveProperty("limit");
        expect(model).not.toHaveProperty("compat");
        expect(model).not.toHaveProperty("thinkingLevelMap");
        expect(model).not.toHaveProperty("input");
      }
    }
  });

  it("keeps providers Command Code users expect", () => {
    expect(preset("OpenRouter")?.settingsConfig.baseURL).toBe(
      "https://openrouter.ai/api",
    );
    expect(preset("DeepSeek")?.settingsConfig.baseURL).toBe(
      "https://api.deepseek.com/v1",
    );
    expect(preset("Ollama")?.settingsConfig.baseURL).toBe(
      "http://localhost:11434/v1",
    );
    expect(Object.keys(preset("Ollama")!.settingsConfig.models)).toContain(
      "qwen3.5",
    );
  });

  it("excludes presets on APIs Command Code does not speak", () => {
    expect(preset("AWS Bedrock")).toBeUndefined();
    for (const entry of commandcodeProviderPresets) {
      expect(
        piProviderPresets.find((candidate) => candidate.name === entry.name)
          ?.settingsConfig.api,
      ).toBe(entry.settingsConfig.api);
    }
  });
});
