import { describe, expect, it } from "vitest";
import {
  CLINE_PROTOCOLS,
  clineConfigSchema,
  clineProviderPresets,
  emptyClineCustomConfig,
  generateClineProviderId,
  isClineCustomConfig,
  validateClineConfig,
  withClineProtocol,
  type ClineProviderConfig,
} from "./clineProviderPresets";
import { piProviderPresets } from "./piProviderPresets";
import { presetGroup } from "@/components/providers/forms/presetGroups";

const find = (name: string) =>
  clineProviderPresets.find((preset) => preset.name === name)!;

const custom = (patch: Partial<ClineProviderConfig> = {}) =>
  ({
    settings: { apiKey: "sk", model: "m1" },
    provider: {
      name: "Relay",
      baseUrl: "https://relay.example.com/v1",
      protocol: "openai-chat",
    },
    models: { m1: { id: "m1", contextWindow: 200000 } },
    ...patch,
  }) as ClineProviderConfig;

describe("clineProviderPresets", () => {
  it("covers every OpenAI- or Anthropic-style catalog vendor plus Cline-native slots", () => {
    const admitted = piProviderPresets.filter(
      (preset) =>
        preset.settingsConfig.api !== "bedrock-converse-stream" &&
        preset.settingsConfig.api !== "google-generative-ai",
    );
    const names = clineProviderPresets.map((preset) => preset.name);
    for (const preset of admitted) expect(names).toContain(preset.name);
    expect(names).not.toContain("AWS Bedrock");
    for (const name of [
      "Custom (OpenAI-compatible)",
      "Custom (Anthropic-compatible)",
      "Anthropic",
      "OpenAI",
      "Google Gemini",
    ]) {
      expect(names).toContain(name);
    }
    expect(new Set(clineProviderPresets.map(presetGroup))).toEqual(
      new Set(["vendor", "thirdparty", "cloud", "local"]),
    );
  });

  it("produces configs the backend accepts", () => {
    for (const preset of clineProviderPresets) {
      const config = clineConfigSchema.parse(preset.settingsConfig);
      if (preset.category === "custom") continue;
      expect(validateClineConfig(config), preset.name).toEqual([]);
    }
  });

  it("maps vendors with a matching Cline provider onto the built-in slot", () => {
    for (const [name, id] of [
      ["DeepSeek", "deepseek"],
      ["Kimi", "moonshot"],
      ["Kimi Global", "moonshot"],
      ["OpenRouter", "openrouter"],
      ["Ollama", "ollama"],
      ["LM Studio", "lmstudio"],
      ["Anthropic", "anthropic"],
      ["OpenAI", "openai-native"],
    ]) {
      const preset = find(name);
      expect(preset.providerKey, name).toBe(id);
      expect(isClineCustomConfig(preset.settingsConfig), name).toBe(false);
    }
    // Regional endpoints stay; otherwise Cline's own default endpoint is used.
    expect(find("Kimi").settingsConfig.settings.baseUrl).toBe(
      "https://api.moonshot.cn/v1",
    );
    expect(find("DeepSeek").settingsConfig.settings.baseUrl).toBeUndefined();
    expect(find("Ollama").settingsConfig.settings.apiKey).toBeUndefined();
    expect(find("DeepSeek").suggestedModels).toContain(
      find("DeepSeek").settingsConfig.settings.model,
    );
  });

  it("declares other vendors as custom providers with Cline's protocols", () => {
    const anthropic = find("PackyCode").settingsConfig;
    expect(anthropic.provider).toMatchObject({
      name: "PackyCode",
      baseUrl: "https://www.packyapi.ai/v1",
      protocol: "anthropic",
      client: "anthropic",
    });
    const kimiCoding = find("Kimi For Coding").settingsConfig.provider!;
    expect(kimiCoding.baseUrl).toBe("https://api.kimi.com/coding/v1");

    const chat = find("APINebula").settingsConfig;
    expect(chat.provider).toMatchObject({
      baseUrl: "https://apinebula.ai/v1",
      protocol: "openai-chat",
    });
    expect(chat.provider!.client).toBeUndefined();
    expect(find("RightCode").settingsConfig.provider!.protocol).toBe(
      "openai-responses",
    );
    const models = Object.entries(chat.models!);
    expect(models.length).toBeGreaterThan(0);
    for (const [key, model] of models) {
      expect(model.id).toBe(key);
      expect(model.contextWindow).toBeGreaterThan(0);
      expect(model.capabilities).toContain("tools");
    }
    expect(chat.settings.model).toBe(chat.provider!.defaultModelId);
    expect(find("NekoCode").settingsConfig.settings.headers).toBeTruthy();
  });
});

describe("Cline provider helpers", () => {
  it("generates stable ccs- ids and avoids collisions", () => {
    expect(generateClineProviderId("My Relay!", [])).toBe("ccs-my-relay");
    expect(generateClineProviderId("Café Ünï", [])).toBe("ccs-cafe-uni");
    expect(generateClineProviderId("模力方舟", [])).toBe("ccs-custom");
    expect(generateClineProviderId("Relay", ["ccs-relay", "ccs-relay-2"])).toBe(
      "ccs-relay-3",
    );
  });

  it("pairs the anthropic and gemini protocols with their clients", () => {
    const provider = { name: "R", baseUrl: "https://r.example/v1" };
    expect(withClineProtocol(provider, "anthropic").client).toBe("anthropic");
    expect(withClineProtocol(provider, "gemini").client).toBe("gemini");
    expect(
      withClineProtocol({ ...provider, client: "anthropic" }, "openai-chat"),
    ).not.toHaveProperty("client");
    expect(
      withClineProtocol({ ...provider, client: "anthropic" }, "gemini").client,
    ).toBe("gemini");
    // A native client unrelated to the protocols is kept.
    expect(
      withClineProtocol({ ...provider, client: "fetch" }, "openai-chat").client,
    ).toBe("fetch");
    expect(emptyClineCustomConfig("anthropic").provider!.client).toBe(
      "anthropic",
    );
    expect(emptyClineCustomConfig("gemini").provider!.client).toBe("gemini");
    expect(CLINE_PROTOCOLS).toContain("openai-chat");
  });

  it("no longer offers openai-r1 but still accepts it from Cline", () => {
    expect(CLINE_PROTOCOLS as readonly string[]).not.toContain("openai-r1");
    expect(
      validateClineConfig(
        custom({
          provider: {
            name: "R1",
            baseUrl: "https://r.example/v1",
            protocol: "openai-r1",
          },
        }),
      ),
    ).toEqual([]);
  });

  it("mirrors the backend validation rules", () => {
    expect(validateClineConfig(custom())).toEqual([]);
    expect(
      validateClineConfig(
        custom({
          provider: {
            name: " ",
            baseUrl: "ftp://relay",
            protocol: "anthropic",
          },
        }),
      ),
    ).toEqual([
      "cline.validation.name",
      "cline.validation.baseUrl",
      "cline.validation.protocolClient",
    ]);
    expect(validateClineConfig(custom({ models: {} }))).toEqual([
      "cline.validation.models",
    ]);
    expect(
      validateClineConfig(
        custom({ models: { m1: { contextWindow: Number.NaN } } }),
      ),
    ).toEqual(["cline.validation.modelNumber"]);
    expect(
      validateClineConfig(
        custom({
          provider: {
            name: "G",
            baseUrl: "https://g.example/v1beta",
            protocol: "gemini",
          },
        }),
      ),
    ).toEqual(["cline.validation.protocolClient"]);
    expect(
      validateClineConfig(custom({ settings: { model: "missing" } })),
    ).toEqual(["cline.validation.selectedModel"]);
    // Built-in providers only check their optional endpoint.
    expect(
      validateClineConfig({ settings: { apiKey: "k", baseUrl: "nope" } }),
    ).toEqual(["cline.validation.baseUrl"]);
    expect(validateClineConfig({ settings: { apiKey: "k" } })).toEqual([]);
  });
});
