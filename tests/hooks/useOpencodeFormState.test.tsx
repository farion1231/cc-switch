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

  return {
    ...hook,
    onSettingsConfigChange,
    getSettingsConfig: () => settingsConfig,
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
});
