import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  hasClaudeOneMMarker,
  setClaudeOneMMarker,
  stripClaudeOneMMarker,
  useModelState,
} from "@/components/providers/forms/hooks/useModelState";

describe("useModelState", () => {
  it("hydrates role models and display names from Claude Code env", () => {
    const settingsConfig = JSON.stringify({
      env: {
        ANTHROPIC_MODEL: "fallback-model",
        ANTHROPIC_SMALL_FAST_MODEL: "legacy-small",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "deepseek-v4-pro",
        ANTHROPIC_DEFAULT_SONNET_MODEL_NAME: "DeepSeek V4 Pro",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "kimi-k2",
        ANTHROPIC_DEFAULT_OPUS_MODEL_NAME: "Kimi K2",
      },
    });

    const { result } = renderHook(() =>
      useModelState({
        settingsConfig,
        onConfigChange: vi.fn(),
      }),
    );

    expect(result.current.claudeModel).toBe("fallback-model");
    expect(result.current.defaultSonnetModel).toBe("deepseek-v4-pro");
    expect(result.current.defaultSonnetModelName).toBe("DeepSeek V4 Pro");
    expect(result.current.defaultOpusModel).toBe("kimi-k2");
    expect(result.current.defaultOpusModelName).toBe("Kimi K2");
    expect(result.current.defaultHaikuModel).toBe("legacy-small");
    expect(result.current.defaultHaikuModelName).toBe("legacy-small");
  });

  it("writes and clears role display-name env fields without changing model mapping", () => {
    let latestConfig = JSON.stringify({
      env: {
        ANTHROPIC_DEFAULT_SONNET_MODEL: "deepseek-v4-pro",
      },
    });
    const onConfigChange = vi.fn((config: string) => {
      latestConfig = config;
    });

    const { result } = renderHook(() =>
      useModelState({
        settingsConfig: latestConfig,
        onConfigChange,
      }),
    );

    act(() => {
      result.current.handleModelChange(
        "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
        "DeepSeek V4 Pro",
      );
    });

    let env = JSON.parse(latestConfig).env;
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("deepseek-v4-pro");
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL_NAME).toBe("DeepSeek V4 Pro");

    act(() => {
      result.current.handleModelChange(
        "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
        "",
      );
    });

    env = JSON.parse(latestConfig).env;
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("deepseek-v4-pro");
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL_NAME).toBeUndefined();
  });

  it("keeps the 1M marker on request models but strips it from fallback display names", () => {
    const settingsConfig = JSON.stringify({
      env: {
        ANTHROPIC_DEFAULT_SONNET_MODEL: "deepseek-v4-pro[1M]",
      },
    });

    const { result } = renderHook(() =>
      useModelState({
        settingsConfig,
        onConfigChange: vi.fn(),
      }),
    );

    expect(result.current.defaultSonnetModel).toBe("deepseek-v4-pro[1M]");
    expect(result.current.defaultSonnetModelName).toBe("deepseek-v4-pro");
  });

  it("normalizes Claude Code 1M markers for UI toggles", () => {
    expect(hasClaudeOneMMarker("deepseek-v4-pro[1m]")).toBe(true);
    expect(hasClaudeOneMMarker("deepseek-v4-pro [1M]  ")).toBe(true);
    expect(stripClaudeOneMMarker("deepseek-v4-pro [1M]  ")).toBe(
      "deepseek-v4-pro",
    );
    expect(setClaudeOneMMarker("deepseek-v4-pro [1M]", false)).toBe(
      "deepseek-v4-pro",
    );
    expect(setClaudeOneMMarker("deepseek-v4-pro", true)).toBe(
      "deepseek-v4-pro[1M]",
    );
  });
});
