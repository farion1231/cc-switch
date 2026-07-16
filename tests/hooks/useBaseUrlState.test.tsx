import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useBaseUrlState } from "@/components/providers/forms/hooks/useBaseUrlState";

describe("useBaseUrlState", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("switching to another Claude preset after a local endpoint edit should not keep the previous base URL", () => {
    vi.useFakeTimers();

    let settingsConfig = JSON.stringify({
      env: {
        ANTHROPIC_BASE_URL: "https://ark.cn-beijing.volces.com/api/coding",
      },
    });
    const onSettingsConfigChange = vi.fn((config: string) => {
      settingsConfig = config;
    });

    const { result, rerender } = renderHook(
      ({ config }) =>
        useBaseUrlState({
          appType: "claude",
          category: "cn_official",
          settingsConfig: config,
          codexConfig: "",
          onSettingsConfigChange,
          onCodexConfigChange: vi.fn(),
        }),
      {
        initialProps: { config: settingsConfig },
      },
    );

    expect(result.current.baseUrl).toBe(
      "https://ark.cn-beijing.volces.com/api/coding",
    );

    act(() => {
      result.current.handleClaudeBaseUrlChange(
        "https://api.longcat.chat/anthropic",
      );
    });

    settingsConfig = JSON.stringify({
      env: {
        ANTHROPIC_BASE_URL: "https://open.bigmodel.cn/api/anthropic",
      },
    });

    rerender({ config: settingsConfig });

    expect(result.current.baseUrl).toBe("https://open.bigmodel.cn/api/anthropic");

    act(() => {
      vi.runAllTimers();
    });
  });
});
