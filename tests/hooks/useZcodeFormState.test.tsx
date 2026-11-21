import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useZcodeFormState } from "@/components/providers/forms/hooks/useZcodeFormState";

const renderZcodeFormState = (
  initialSettingsConfig: Record<string, unknown>,
) => {
  let settingsConfig = JSON.stringify(initialSettingsConfig);
  const onSettingsConfigChange = vi.fn((nextConfig: string) => {
    settingsConfig = nextConfig;
  });

  const hook = renderHook(() =>
    useZcodeFormState({
      appId: "zcode",
      initialData: { settingsConfig: initialSettingsConfig },
      onSettingsConfigChange,
    }),
  );

  return {
    ...hook,
    onSettingsConfigChange,
    getSettingsConfig: () => settingsConfig,
  };
};

describe("useZcodeFormState", () => {
  it("preserves extra top-level fields across structured edits", () => {
    const { result, getSettingsConfig } = renderZcodeFormState({
      name: "BigModel",
      kind: "anthropic",
      enabled: true,
      source: "custom",
      options: {
        baseURL: "https://open.bigmodel.cn/api/anthropic",
        apiKey: "key-1",
      },
      models: {},
      // 非白名单顶层字段：后端 zcode_provider_config_preserves_extra_top_level_fields
      // 明确支持往返，编辑凭据时不得丢掉。
      priority: 300,
      tags: ["production"],
    });

    act(() => result.current.handleZcodeApiKeyChange("key-2"));

    const emitted = JSON.parse(getSettingsConfig());
    expect(emitted.options.apiKey).toBe("key-2");
    expect(emitted.priority).toBe(300);
    expect(emitted.tags).toEqual(["production"]);
    expect(emitted.enabled).toBe(true);
    expect(emitted.source).toBe("custom");
  });

  it("drops extras only when a reset installs a fresh preset config", () => {
    const { result, getSettingsConfig } = renderZcodeFormState({
      options: { apiKey: "key-1" },
      priority: 300,
    });

    act(() =>
      result.current.resetZcodeState({
        kind: "anthropic",
        options: { baseURL: "https://example.com", apiKey: "preset-key" },
        models: {},
      }),
    );
    act(() => result.current.handleZcodeNameChange("Preset"));

    const emitted = JSON.parse(getSettingsConfig());
    expect(emitted.name).toBe("Preset");
    expect(emitted.options.apiKey).toBe("preset-key");
    expect(emitted.priority).toBeUndefined();
  });

  it("keeps enabled/source untouched when only credentials change", () => {
    const { result, getSettingsConfig } = renderZcodeFormState({
      name: "Disabled provider",
      enabled: false,
      source: "preset",
      options: { apiKey: "key-1" },
    });

    act(() => result.current.handleZcodeApiKeyChange("key-2"));

    const emitted = JSON.parse(getSettingsConfig());
    expect(emitted.enabled).toBe(false);
    expect(emitted.source).toBe("preset");
    expect(emitted.options.apiKey).toBe("key-2");
  });

  it("derives apiKeyRequired from the real initial config, not the default template", () => {
    // 新建供应商：没有初始 settingsConfig。parseZcodeConfig(undefined) 会回退到
    // 默认模板（options 里带 apiKey: ""），不得据此判定「初始配置里有 key」
    const fresh = renderHook(() =>
      useZcodeFormState({
        appId: "zcode",
        onSettingsConfigChange: vi.fn(),
      }),
    );
    expect(fresh.result.current.zcodeApiKeyRequired).toBe(false);

    // 初始配置里确实写了 apiKey → 必填
    const withKey = renderZcodeFormState({ options: { apiKey: "key-1" } });
    expect(withKey.result.current.zcodeApiKeyRequired).toBe(true);

    // 初始配置没写 apiKey（免鉴权自定义端点）→ 非必填
    const withoutKey = renderZcodeFormState({
      options: { baseURL: "https://example.com" },
    });
    expect(withoutKey.result.current.zcodeApiKeyRequired).toBe(false);
  });
});
