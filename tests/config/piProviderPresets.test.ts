import { describe, expect, it } from "vitest";
import {
  getPiModelCatalogReference,
  piModelCatalog,
} from "@/config/piModelCatalog";
import { piProviderPresets } from "@/config/piProviderPresets";

describe("Pi provider presets", () => {
  it("owns a broad provider catalog without OpenCode-only templates", () => {
    const names = piProviderPresets.map((preset) => preset.name);

    expect(piProviderPresets.length).toBeGreaterThanOrEqual(50);
    expect(names).toEqual(
      expect.arrayContaining(["Kimi", "DeepSeek", "OpenRouter", "AWS Bedrock"]),
    );
    expect(names).not.toContain("Oh My OpenCode");
    expect(names).not.toContain("Oh My OpenCode Slim");
  });

  it("uses distinct managed keys instead of shadowing Pi-native providers", () => {
    const keys = piProviderPresets.map((preset) => preset.providerKey);

    expect(new Set(keys).size).toBe(keys.length);
    expect(keys.every((key) => key.startsWith("cc-switch-"))).toBe(true);
  });

  it("only supplies configuration defaults, never a second gateway decision", () => {
    for (const preset of piProviderPresets) {
      expect("allowGateway" in preset).toBe(false);
      expect(preset.settingsConfig.baseUrl).toMatch(/^https?:\/\//);
      expect(preset.settingsConfig.api).not.toBe("");
      expect(preset.settingsConfig.apiKey).toBe("");
      expect(preset.settingsConfig.models.length).toBeGreaterThan(0);
      const modelIds = preset.settingsConfig.models.map((model) => model.id);
      expect(new Set(modelIds).size).toBe(modelIds.length);
      for (const model of preset.settingsConfig.models) {
        expect(model.id).not.toBe("");
        expect(model.name).not.toBe("");
        expect(typeof model.reasoning).toBe("boolean");
        expect(model.input.length).toBeGreaterThan(0);
        expect(model.input.every((input) => ["text", "image"].includes(input)));
        expect(model.contextWindow).toBeGreaterThan(0);
        expect(model.maxTokens).toBeGreaterThan(0);
        const reference = getPiModelCatalogReference(model);
        if (!reference) {
          throw new Error(`${preset.name}/${model.id} has no catalog profile`);
        }
        expect(piModelCatalog).toHaveProperty(reference.catalogKey);
        expect(JSON.parse(JSON.stringify(model))).not.toHaveProperty(
          "catalogKey",
        );
      }
    }
  });

  it("uses Anthropic roots that produce Pi request paths", () => {
    const requestUrls = Object.fromEntries(
      piProviderPresets
        .filter((preset) => preset.settingsConfig.api === "anthropic-messages")
        .map((preset) => {
          const base = new URL(preset.settingsConfig.baseUrl);
          const basePath = base.pathname.replace(/\/+$/, "");
          base.pathname = `${basePath}/v1/messages`;
          return [preset.name, base.toString()];
        }),
    );

    expect(requestUrls).toMatchObject({
      "Kimi For Coding": "https://api.kimi.com/coding/v1/messages",
      PackyCode: "https://www.packyapi.ai/v1/messages",
      AICodeMirror: "https://api.aicodemirror.ai/api/claudecode/v1/messages",
      OpenRouter: "https://openrouter.ai/api/v1/messages",
    });
  });

  it("stores Pi-native API formats in its own catalog", () => {
    expect(
      Object.fromEntries(
        piProviderPresets.map((preset) => [
          preset.name,
          preset.settingsConfig.api,
        ]),
      ),
    ).toMatchObject({
      Kimi: "openai-completions",
      "Kimi For Coding": "anthropic-messages",
      RightCode: "openai-responses",
      "AWS Bedrock": "bedrock-converse-stream",
    });
  });

  it("uses Pi's 272K context value for every GPT-5.6 Sol preset", () => {
    const models = piProviderPresets.flatMap((preset) =>
      preset.settingsConfig.models.filter(
        (model) => model.id === "gpt-5.6-sol",
      ),
    );

    expect(models.length).toBeGreaterThan(0);
    expect(models.every((model) => model.contextWindow === 272_000)).toBe(true);
  });

  it("keeps provider-specific OpenAI compatibility metadata", () => {
    const preset = (name: string) => {
      const found = piProviderPresets.find((item) => item.name === name);
      if (!found) throw new Error(`Missing Pi preset: ${name}`);
      return found;
    };
    const model = (presetName: string, id: string) => {
      const found = preset(presetName).settingsConfig.models.find(
        (item) => item.id === id,
      );
      if (!found) throw new Error(`Missing Pi model: ${presetName}/${id}`);
      return found;
    };

    expect(model("Kimi", "kimi-k3").compat).toEqual({
      supportsStore: false,
      supportsDeveloperRole: false,
      supportsReasoningEffort: true,
      maxTokensField: "max_tokens",
      supportsStrictMode: false,
      thinkingFormat: "openai",
      requiresReasoningContentOnAssistantMessages: true,
      deferredToolsMode: "kimi",
    });

    const openCodeBase = {
      supportsStore: false,
      supportsDeveloperRole: false,
      maxTokensField: "max_tokens",
    };
    expect(model("OpenCode Go", "glm-5.2").compat).toEqual(openCodeBase);
    expect(model("OpenCode Go", "kimi-k2.7-code").compat).toEqual(openCodeBase);
    expect(model("OpenCode Go", "mimo-v2.5-pro").compat).toEqual(openCodeBase);
    for (const id of ["deepseek-v4-pro", "deepseek-v4-flash"]) {
      expect(model("OpenCode Go", id).compat).toEqual({
        ...openCodeBase,
        requiresReasoningContentOnAssistantMessages: true,
        thinkingFormat: "deepseek",
      });
    }

    for (const id of ["mimo-v2.5-pro", "mimo-v2.5"]) {
      expect(model("Xiaomi MiMo", id).compat).toEqual({
        requiresReasoningContentOnAssistantMessages: true,
        thinkingFormat: "deepseek",
      });
    }
  });
});
