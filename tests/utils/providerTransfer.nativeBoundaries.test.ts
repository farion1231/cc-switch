import { beforeAll, describe, expect, it } from "vitest";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";
import { kimiProviderPresets } from "@/config/kimiProviderPresets";
import {
  extractProviderTransferFields,
  prepareProviderTransfer,
} from "@/utils/providerTransfer";
import type { Provider } from "@/types";

beforeAll(() => {
  i18n.addResourceBundle("zh", "translation", zh, true, true);
});
const source = (model: Record<string, unknown>): Provider => ({
  id: "source",
  name: "Example",
  settingsConfig: {
    baseUrl: "https://example.test/v1",
    apiKey: "test-key",
    api: "anthropic-messages",
    models: [
      { id: "example", contextWindow: 128000, maxTokens: 8192, ...model },
    ],
  },
});

describe("native transfer boundaries", () => {
  it.each([" opaque-model ", "opaque-model[1m]"])(
    "preserves opaque Pi ID %s",
    async (id) => {
      const fields = extractProviderTransferFields(source({ id }), "pi");
      expect(fields.models[0].id).toBe(id);
      expect(fields.models[0].supports1m).toBeUndefined();
      const { provider } = await prepareProviderTransfer(fields, "pi");
      expect(provider.settingsConfig.models[0].id).toBe(id);
    },
  );
  it.each([
    { api: "openai-completions" },
    { baseUrl: "https://other.test/v1" },
  ])("rejects model-specific connection override %j", (override) => {
    expect(() => extractProviderTransferFields(source(override), "pi")).toThrow(
      i18n.t("provider.transfer.errors.modelConnectionOverride"),
    );
  });
  it("accepts redundant model connection fields", () => {
    expect(() =>
      extractProviderTransferFields(
        source({
          api: "anthropic-messages",
          baseUrl: "https://example.test/v1",
        }),
        "pi",
      ),
    ).not.toThrow();
  });
  it("retains Kimi native defaults without retaining provider identity", async () => {
    const preset = kimiProviderPresets.find((p) => p.name === "OpenCode Go")!;
    const defaults = Object.values(preset.settingsConfig.models).find(
      (m) => m.model === "glm-5.3",
    )!;
    const fields = {
      sourceApp: "claude" as const,
      name: "Example",
      baseUrl: "https://opencode.ai/zen/go",
      apiKey: "test-key",
      apiFormat: "anthropic" as const,
      models: [{ id: "glm-5.3" }],
    };
    const before = JSON.stringify(preset);
    const result = await prepareProviderTransfer(fields, "kimi");
    const model = result.provider.settingsConfig.models["glm-5.3"];
    expect(model.max_context_size).toBe(defaults.max_context_size);
    expect(model.capabilities).toEqual(defaults.capabilities);
    expect(model).not.toHaveProperty("provider");
    expect(result.warnings).not.toContain(
      i18n.t("provider.transfer.warnings.kimiContext", { size: 262144 }),
    );
    expect(JSON.stringify(preset)).toBe(before);
    const overridden = await prepareProviderTransfer(
      {
        ...fields,
        models: [{ id: "glm-5.3", contextWindow: 64000, name: "Mine" }],
      },
      "kimi",
    );
    expect(overridden.provider.settingsConfig.models["glm-5.3"]).toMatchObject({
      max_context_size: 64000,
      display_name: "Mine",
      capabilities: defaults.capabilities,
    });
  });
});
