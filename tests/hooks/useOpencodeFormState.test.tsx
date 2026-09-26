import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useOpencodeFormState } from "@/components/providers/forms/hooks/useOpencodeFormState";

const renderOpencodeFormState = (
  initialSettingsConfig: Record<string, unknown>,
) => {
  let settingsConfig = JSON.stringify(initialSettingsConfig);
  const onSettingsConfigChange = vi.fn((nextConfig: string) => {
    settingsConfig = nextConfig;
  });

  const hook = renderHook(() =>
    useOpencodeFormState({
      appId: "opencode",
      initialData: { settingsConfig: initialSettingsConfig },
      onSettingsConfigChange,
      getSettingsConfig: () => settingsConfig,
    }),
  );

  const setSettingsConfig = (nextConfig: Record<string, unknown>) => {
    settingsConfig = JSON.stringify(nextConfig);
    hook.rerender();
  };

  return {
    ...hook,
    onSettingsConfigChange,
    getSettingsConfig: () => settingsConfig,
    setSettingsConfig,
  };
};

describe("useOpencodeFormState", () => {
  it("hydrates provider headers from options", () => {
    const { result } = renderOpencodeFormState({
      npm: "@ai-sdk/openai-compatible",
      options: {
        headers: {
          "HTTP-Referer": "https://cc-switch.app",
          "X-Title": "CC Switch",
        },
      },
      models: {},
    });

    expect(result.current.opencodeHeaders).toEqual({
      "HTTP-Referer": "https://cc-switch.app",
      "X-Title": "CC Switch",
    });
  });

  it("writes provider headers to options", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai-compatible",
      options: {},
      models: {},
    });

    act(() => {
      result.current.handleOpencodeHeadersChange({
        "X-Title": "CC Switch",
      });
    });

    expect(JSON.parse(getSettingsConfig()).options.headers).toEqual({
      "X-Title": "CC Switch",
    });
  });

  it("removes options.headers when all provider headers are removed", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai-compatible",
      options: {
        headers: {
          "X-Title": "CC Switch",
        },
      },
      models: {},
    });

    act(() => {
      result.current.handleOpencodeHeadersChange({});
    });

    expect(JSON.parse(getSettingsConfig()).options.headers).toBeUndefined();
  });

  it("preserves legitimate headers whose names start with header-", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai-compatible",
      options: {
        headers: {
          "header-version": "v1",
          "X-Title": "Old",
        },
      },
      models: {},
    });

    act(() => {
      result.current.handleOpencodeHeadersChange({
        "header-version": "v1",
        "X-Title": "New",
      });
    });

    expect(JSON.parse(getSettingsConfig()).options.headers).toEqual({
      "header-version": "v1",
      "X-Title": "New",
    });
  });

  it("preserves legitimate options whose names start with option-", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai-compatible",
      options: {
        "option-mode": "legacy",
        timeout: 100,
      },
      models: {},
    });

    act(() => {
      result.current.handleOpencodeExtraOptionsChange({
        "option-mode": "legacy",
        timeout: "200",
        "draft-option:123": "",
      });
    });

    expect(JSON.parse(getSettingsConfig()).options).toEqual({
      "option-mode": "legacy",
      timeout: 200,
    });
  });

  it("syncs a known MiniMax base URL when switching to Anthropic", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai-compatible",
      options: { baseURL: "https://api.minimax.cn/v1" },
      models: {},
    });

    act(() => {
      result.current.handleOpencodeNpmChange("@ai-sdk/anthropic");
    });

    expect(result.current.opencodeNpm).toBe("@ai-sdk/anthropic");
    expect(result.current.opencodeBaseUrl).toBe(
      "https://api.minimax.cn/anthropic",
    );
    expect(JSON.parse(getSettingsConfig())).toMatchObject({
      npm: "@ai-sdk/anthropic",
      options: { baseURL: "https://api.minimax.cn/anthropic" },
    });
  });

  it("syncs a known MiniMax base URL back to the OpenAI path", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/anthropic",
      options: { baseURL: "https://api.minimax.io/anthropic" },
      models: {},
    });

    act(() => {
      result.current.handleOpencodeNpmChange("@ai-sdk/openai-compatible");
    });

    expect(result.current.opencodeNpm).toBe("@ai-sdk/openai-compatible");
    expect(result.current.opencodeBaseUrl).toBe("https://api.minimax.io/v1");
    expect(JSON.parse(getSettingsConfig())).toMatchObject({
      npm: "@ai-sdk/openai-compatible",
      options: { baseURL: "https://api.minimax.io/v1" },
    });
  });

  it("never overwrites a custom base URL when changing API format", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai-compatible",
      options: { baseURL: "https://relay.example.com/v1" },
      models: {},
    });

    act(() => {
      result.current.handleOpencodeNpmChange("@ai-sdk/anthropic");
    });

    expect(result.current.opencodeNpm).toBe("@ai-sdk/anthropic");
    expect(result.current.opencodeBaseUrl).toBe("https://relay.example.com/v1");
    expect(JSON.parse(getSettingsConfig())).toMatchObject({
      npm: "@ai-sdk/anthropic",
      options: { baseURL: "https://relay.example.com/v1" },
    });
  });

  it.each([
    [
      "custom relay",
      "https://relay.example.com/anthropic",
      "https://relay.example.com/anthropic",
    ],
    [
      "URL with query",
      "https://api.minimax.cn/v1?tenant=example",
      "https://api.minimax.cn/v1?tenant=example",
    ],
    [
      "international MiniMax endpoint",
      "https://api.minimax.io/anthropic",
      "https://api.minimax.io/anthropic",
    ],
  ] as const)(
    "uses the latest JSON-edited base URL for %s",
    (_caseName, editedBaseUrl, expectedBaseUrl) => {
      const { result, getSettingsConfig, setSettingsConfig } =
        renderOpencodeFormState({
          npm: "@ai-sdk/openai-compatible",
          options: { baseURL: "https://api.minimax.cn/v1" },
          models: {},
        });

      act(() => {
        setSettingsConfig({
          npm: "@ai-sdk/openai-compatible",
          options: { baseURL: editedBaseUrl },
          models: {},
        });
      });

      act(() => {
        result.current.handleOpencodeNpmChange("@ai-sdk/anthropic");
      });

      expect(result.current.opencodeBaseUrl).toBe(expectedBaseUrl);
      expect(JSON.parse(getSettingsConfig())).toMatchObject({
        npm: "@ai-sdk/anthropic",
        options: { baseURL: expectedBaseUrl },
      });
    },
  );
});
