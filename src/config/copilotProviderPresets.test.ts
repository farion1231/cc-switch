import { describe, expect, it } from "vitest";
import {
  COPILOT_API_FORMATS,
  applyCopilotApiFormat,
  copilotApiFormatOf,
  copilotByokConfigOf,
  copilotOfficialPreset,
  copilotPresetProviderId,
  copilotProviderPresets,
  isCopilotByok,
  isCopilotOfficialCard,
  isValidCopilotProviderId,
} from "./copilotProviderPresets";

describe("copilotProviderPresets", () => {
  it("translates catalog presets into providers.json entries", () => {
    expect(copilotProviderPresets.length).toBeGreaterThan(5);
    const ids = new Set<string>();
    for (const preset of copilotProviderPresets) {
      const { provider, models } = preset.settingsConfig;
      expect(provider.baseUrl).toMatch(/^https?:\/\//);
      expect(["openai", "anthropic"]).toContain(provider.type);
      if (provider.type === "anthropic") {
        expect(provider.wireApi).toBeUndefined();
      } else {
        expect(["completions", "responses"]).toContain(provider.wireApi);
      }
      // providers.json 的条目不带 name，模型不带 provider（写文件时补上卡片 id）
      expect(provider).not.toHaveProperty("name");
      expect(models.length).toBeGreaterThan(0);
      for (const model of models) {
        expect(model.id).toBeTruthy();
        expect(model).not.toHaveProperty("provider");
      }
      expect(isValidCopilotProviderId(preset.providerKey)).toBe(true);
      expect(preset.providerKey).not.toMatch(/^cc-switch-/);
      ids.add(preset.providerKey);
    }
    expect(ids.size).toBe(copilotProviderPresets.length);
  });

  it("maps Pi model limits onto Copilot model fields", () => {
    const withLimits = copilotProviderPresets.flatMap(
      (preset) => preset.settingsConfig.models,
    );
    expect(withLimits.some((m) => m.maxContextWindowTokens! > 0)).toBe(true);
    expect(withLimits.some((m) => m.maxOutputTokens! > 0)).toBe(true);
    const kimi = copilotProviderPresets.find(
      (preset) => preset.providerKey === "kimi",
    )!;
    const code = kimi.settingsConfig.models.find(
      ({ id }) => id === "kimi-k2.7-code",
    );
    expect(code?.maxContextWindowTokens).toBe(262144);
    expect(code?.maxOutputTokens).toBeUndefined();
    for (const model of withLimits) {
      if (model.maxContextWindowTokens && model.maxOutputTokens)
        expect(model.maxOutputTokens).toBeLessThan(
          model.maxContextWindowTokens,
        );
    }
    expect(copilotPresetProviderId("cc-switch-deep-seek")).toBe("deep-seek");
  });

  it("local presets get a placeholder key for servers without auth", () => {
    const local = copilotProviderPresets.filter((p) => p.category === "local");
    expect(local.length).toBeGreaterThan(0);
    for (const preset of local) {
      expect(preset.settingsConfig.provider.apiKey).toBeTruthy();
    }
  });

  it("tells official cards from BYOK cards by the provider node", () => {
    expect(isCopilotByok(copilotOfficialPreset.settingsConfig)).toBe(false);
    expect(
      isCopilotByok({ provider: { baseUrl: "http://x/v1" }, models: [] }),
    ).toBe(true);
    // 旧 env 形状只为显示仍认得出
    expect(
      isCopilotByok({ env: { COPILOT_PROVIDER_BASE_URL: " http://x/v1 " } }),
    ).toBe(true);
    expect(
      isCopilotOfficialCard({
        category: "custom",
        settingsConfig: { env: { COPILOT_MODEL: "gpt-5.4" } },
      }),
    ).toBe(true);
    expect(
      isCopilotOfficialCard({
        category: "official",
        settingsConfig: { provider: { baseUrl: "http://x/v1" } },
      }),
    ).toBe(true);
  });

  it("validates card ids as Copilot provider names", () => {
    expect(isValidCopilotProviderId("deepseek")).toBe(true);
    expect(isValidCopilotProviderId("my.relay_1")).toBe(true);
    expect(isValidCopilotProviderId("")).toBe(false);
    expect(isValidCopilotProviderId("a/b")).toBe(false);
    expect(isValidCopilotProviderId("a b")).toBe(false);
    expect(isValidCopilotProviderId("x".repeat(65))).toBe(false);
  });

  it("reads legacy env cards in the new shape", () => {
    expect(
      copilotByokConfigOf({
        env: {
          COPILOT_PROVIDER_BASE_URL: "https://api.example.com/v1",
          COPILOT_PROVIDER_TYPE: "azure",
          COPILOT_PROVIDER_WIRE_API: "responses",
          COPILOT_PROVIDER_API_KEY: "sk-test",
          COPILOT_PROVIDER_AZURE_API_VERSION: "2025-04-01",
          COPILOT_PROVIDER_HEADERS: "X-A: 1\nX-B: two",
          COPILOT_PROVIDER_WIRE_MODEL: "gpt-upstream",
          COPILOT_PROVIDER_MAX_OUTPUT_TOKENS: "8192",
          COPILOT_MODEL: "gpt-5.4",
        },
        reasoningEffort: "high",
      }),
    ).toEqual({
      provider: {
        baseUrl: "https://api.example.com/v1",
        type: "azure",
        wireApi: "responses",
        apiKey: "sk-test",
        azure: { apiVersion: "2025-04-01" },
        headers: { "X-A": "1", "X-B": "two" },
      },
      models: [
        { id: "gpt-5.4", wireModel: "gpt-upstream", maxOutputTokens: 8192 },
      ],
      reasoningEffort: "high",
    });
  });

  it("one API format sets both the protocol and the wire API", () => {
    expect(copilotApiFormatOf({})).toBe("openai-completions");
    expect(copilotApiFormatOf({ type: "azure" })).toBe("azure-completions");
    const responses = applyCopilotApiFormat(
      { baseUrl: "u", transport: "websockets" },
      "openai-responses",
    );
    expect(responses).toEqual({
      baseUrl: "u",
      transport: "websockets",
      type: "openai",
      wireApi: "responses",
    });
    // websockets 只能配 responses，Azure 设置只属于 Azure
    const anthropic = applyCopilotApiFormat(
      { ...responses, azure: { apiVersion: "x" } },
      "anthropic",
    );
    expect(anthropic).toEqual({ baseUrl: "u", type: "anthropic" });
    for (const format of COPILOT_API_FORMATS) {
      expect(
        copilotApiFormatOf(applyCopilotApiFormat({ baseUrl: "" }, format.id)),
      ).toBe(format.id);
    }
    for (const preset of copilotProviderPresets) {
      const { provider } = preset.settingsConfig;
      const id = copilotApiFormatOf(provider);
      expect(applyCopilotApiFormat(provider, id)).toEqual(provider);
    }
  });
});
