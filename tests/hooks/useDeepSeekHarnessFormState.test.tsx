import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useDeepSeekHarnessFormState } from "@/components/providers/forms/hooks/useDeepSeekHarnessFormState";
import {
  isDshOfficial,
  isValidCredentialRef,
  normalizeDshModels,
  pruneDshModelRows,
  withDshRowKeys,
} from "@/components/providers/forms/helpers/deepseekHarnessFormUtils";

interface RenderOptions {
  providerId?: string;
  category?: string;
  meta?: { dshCurrentModel?: string; providerType?: string };
}

const renderDshFormState = (
  initialSettingsConfig: Record<string, unknown>,
  options: RenderOptions = {},
) => {
  let settingsConfig = JSON.stringify(initialSettingsConfig);
  const onSettingsConfigChange = vi.fn((nextConfig: string) => {
    settingsConfig = nextConfig;
  });

  const hook = renderHook(() =>
    useDeepSeekHarnessFormState({
      appId: "deepseek-harness",
      initialData: {
        settingsConfig: initialSettingsConfig,
        category: options.category,
        meta: options.meta,
      },
      providerId: options.providerId,
      onSettingsConfigChange,
      getSettingsConfig: () => settingsConfig,
    }),
  );

  return {
    ...hook,
    onSettingsConfigChange,
    getSettingsConfig: () => settingsConfig,
  };
};

const OFFICIAL_CONFIG = {
  apiKey: "sk-official",
  baseURL: "https://api.deepseek.com",
  profile: "desktop",
  models: [{ id: "deepseek-v4-pro", name: "DeepSeek-V4-Pro" }],
};

const CUSTOM_CONFIG = {
  displayName: "Company Gateway",
  api: "openai-completions",
  baseURL: "https://company.example/v1",
  apiKeyEnv: "COMPANY_API_KEY",
  apiKey: "sk-custom",
  models: [{ id: "deepseek-v4-pro", name: "DeepSeek V4 Pro" }],
};

describe("useDeepSeekHarnessFormState", () => {
  it("hydrates the official provider shape", () => {
    const { result } = renderDshFormState(OFFICIAL_CONFIG, {
      providerId: "deepseek-official",
      category: "official",
    });

    expect(result.current.dshIsOfficial).toBe(true);
    expect(result.current.dshProviderKey).toBe("deepseek-official");
    expect(result.current.dshApiKey).toBe("sk-official");
    expect(result.current.dshApiKeyEnv).toBe("DEEPSEEK_API_KEY");
    expect(result.current.dshBaseUrl).toBe("https://api.deepseek.com");
    expect(result.current.dshApi).toBe("openai-completions");
    expect(result.current.dshModels).toEqual([
      {
        id: "deepseek-v4-pro",
        name: "DeepSeek-V4-Pro",
        rowKey: expect.any(String),
      },
    ]);
  });

  it("hydrates a custom pi-ai provider", () => {
    const { result } = renderDshFormState(CUSTOM_CONFIG, {
      category: "custom",
      meta: { dshCurrentModel: "deepseek-v4-pro" },
    });

    expect(result.current.dshIsOfficial).toBe(false);
    expect(result.current.dshProviderKey).toBe("");
    expect(result.current.dshApiKeyEnv).toBe("COMPANY_API_KEY");
    expect(result.current.dshApi).toBe("openai-completions");
    expect(result.current.dshDefaultModel).toBe("deepseek-v4-pro");
  });

  it("honors meta.providerType over category and credential ref", () => {
    const officialByType = renderDshFormState(
      { ...CUSTOM_CONFIG, apiKeyEnv: "DEEPSEEK_API_KEY" },
      {
        providerId: "company-gateway",
        category: "custom",
        meta: { providerType: "dsh_deepseek" },
      },
    );
    expect(officialByType.result.current.dshIsOfficial).toBe(true);

    const customByType = renderDshFormState(OFFICIAL_CONFIG, {
      providerId: "deepseek-official",
      category: "official",
      meta: { providerType: "dsh_pi_ai" },
    });
    expect(customByType.result.current.dshIsOfficial).toBe(false);
  });

  it("defaults the credential ref for a custom provider", () => {
    const { result } = renderDshFormState({}, { category: "custom" });

    expect(result.current.dshApiKeyEnv).toBe("CUSTOM_DSH_API_KEY");
    expect(result.current.dshBaseUrl).toBe("");
    expect(result.current.dshApi).toBe("openai-completions");
    expect(result.current.dshModels).toEqual([]);
  });

  it("writes base URL, API key and models back with the custom shape", () => {
    const { result, getSettingsConfig } = renderDshFormState(CUSTOM_CONFIG, {
      category: "custom",
    });

    act(() => {
      result.current.handleDshBaseUrlChange("https://gateway.example/v1/");
    });
    let parsed = JSON.parse(getSettingsConfig());
    expect(parsed.baseURL).toBe("https://gateway.example/v1");
    expect(parsed.displayName).toBe("Company Gateway");
    expect(parsed.apiKeyEnv).toBe("COMPANY_API_KEY");

    act(() => {
      result.current.handleDshApiKeyChange("secret");
    });
    parsed = JSON.parse(getSettingsConfig());
    expect(parsed.apiKey).toBe("secret");

    act(() => {
      result.current.handleDshModelsChange([{ id: "glm-5", name: "GLM-5" }]);
    });
    parsed = JSON.parse(getSettingsConfig());
    expect(parsed.models).toEqual([{ id: "glm-5", name: "GLM-5" }]);
  });

  it("keeps the official shape when updating the official provider", () => {
    const { result, getSettingsConfig } = renderDshFormState(OFFICIAL_CONFIG, {
      providerId: "deepseek-official",
      category: "official",
    });

    act(() => {
      result.current.handleDshApiKeyChange("sk-next");
    });

    const parsed = JSON.parse(getSettingsConfig());
    expect(parsed).toMatchObject({
      apiKey: "sk-next",
      baseURL: "https://api.deepseek.com",
      profile: "desktop",
    });
    expect(parsed).not.toHaveProperty("apiKeyEnv");
    expect(parsed).not.toHaveProperty("displayName");
  });

  it("uppercases the credential ref on input", () => {
    const { result, getSettingsConfig } = renderDshFormState(CUSTOM_CONFIG, {
      category: "custom",
    });

    act(() => {
      result.current.handleDshApiKeyEnvChange("my_key");
    });

    expect(result.current.dshApiKeyEnv).toBe("MY_KEY");
    expect(JSON.parse(getSettingsConfig()).apiKeyEnv).toBe("MY_KEY");
  });

  it("updates the default model without touching settingsConfig", () => {
    const { result, onSettingsConfigChange } = renderDshFormState(CUSTOM_CONFIG, {
      category: "custom",
    });

    act(() => {
      result.current.handleDshDefaultModelChange("glm-5");
    });

    expect(result.current.dshDefaultModel).toBe("glm-5");
    expect(onSettingsConfigChange).not.toHaveBeenCalled();
  });

  it("resets to the official shape and back to custom defaults", () => {
    const { result } = renderDshFormState(CUSTOM_CONFIG, { category: "custom" });

    act(() => {
      result.current.resetDshState(OFFICIAL_CONFIG, true);
    });
    expect(result.current.dshIsOfficial).toBe(true);
    expect(result.current.dshProviderKey).toBe("deepseek-official");
    expect(result.current.dshApiKeyEnv).toBe("DEEPSEEK_API_KEY");
    expect(result.current.dshBaseUrl).toBe("https://api.deepseek.com");

    act(() => {
      result.current.resetDshState();
    });
    expect(result.current.dshIsOfficial).toBe(false);
    expect(result.current.dshProviderKey).toBe("");
    expect(result.current.dshApiKeyEnv).toBe("CUSTOM_DSH_API_KEY");
    expect(result.current.dshModels).toEqual([]);
    expect(result.current.dshDefaultModel).toBe("");
  });

  it("ignores a different app id", () => {
    const { result } = renderHook(() =>
      useDeepSeekHarnessFormState({
        appId: "opencode",
        initialData: { settingsConfig: OFFICIAL_CONFIG, category: "official" },
        onSettingsConfigChange: vi.fn(),
        getSettingsConfig: () => "",
      }),
    );

    expect(result.current.dshIsOfficial).toBe(false);
    expect(result.current.dshProviderKey).toBe("");
    expect(result.current.dshModels).toEqual([]);
  });
});

describe("deepseekHarnessFormUtils", () => {
  it("detects official providers by the authoritative providerType", () => {
    // providerType wins over every other signal, in both directions.
    expect(
      isDshOfficial("custom", "custom", undefined, "dsh_deepseek"),
    ).toBe(true);
    expect(
      isDshOfficial("deepseek-official", "official", undefined, "dsh_pi_ai"),
    ).toBe(false);
    // Falls back to the fixed id and official category.
    expect(isDshOfficial("deepseek-official", undefined, undefined)).toBe(true);
    expect(isDshOfficial(undefined, "official", undefined)).toBe(true);
    // The reserved credential ref is not an official signal.
    expect(
      isDshOfficial("custom", "custom", { apiKeyEnv: "DEEPSEEK_API_KEY" }),
    ).toBe(false);
    expect(
      isDshOfficial("custom", "custom", { apiKeyEnv: "DEEPSEEK_API_KEY" }, "dsh_pi_ai"),
    ).toBe(false);
  });

  it("validates credential references", () => {
    expect(isValidCredentialRef("CUSTOM_DSH_API_KEY")).toBe(true);
    expect(isValidCredentialRef("A_1")).toBe(true);
    expect(isValidCredentialRef("_PRIVATE")).toBe(true);
    expect(isValidCredentialRef("lower")).toBe(false);
    expect(isValidCredentialRef("1ABC")).toBe(false);
    expect(isValidCredentialRef("has-dash")).toBe(false);
  });

  it("normalizes model arrays from objects and strings", () => {
    expect(
      normalizeDshModels([
        { id: "a", name: "A", contextWindow: 1000 },
        "b",
        { id: "  pad  " },
        "  spaced  ",
        "",
        null,
        42,
      ]),
    ).toEqual([
      { id: "a", name: "A", contextWindow: 1000 },
      { id: "b", name: undefined, contextWindow: undefined },
      { id: "pad", name: undefined, contextWindow: undefined },
      { id: "spaced", name: undefined, contextWindow: undefined },
    ]);
    expect(normalizeDshModels("nope")).toEqual([]);
  });

  it("prunes blank rows, keeps string-form models, and trims ids", () => {
    const config = {
      displayName: "Gateway",
      models: [
        { id: "" },
        { id: "  real  ", name: "Real" },
        "  str-model  ",
        "",
        null,
        42,
      ],
    };
    // Bare-id strings are valid native catalog entries (the Rust
    // model_id_of accepts them), so the prune keeps them — trimmed.
    expect(pruneDshModelRows(config)).toEqual({
      displayName: "Gateway",
      models: [{ id: "real", name: "Real" }, "str-model"],
    });
    // No pruning or trimming needed: the same object reference comes back.
    const clean = { models: [{ id: "only" }, "plain"] };
    expect(pruneDshModelRows(clean)).toBe(clean);
    expect(pruneDshModelRows({})).toEqual({});
  });

  it("assigns stable client-only row keys without sharing them", () => {
    const rows = withDshRowKeys([
      { id: "a", rowKey: "kept" },
      { id: "b" },
    ]);
    expect(rows[0].rowKey).toBe("kept");
    expect(rows[1].rowKey).toMatch(/^dsh-row-/);
    expect(rows[0].rowKey).not.toBe(rows[1].rowKey);
  });
});
