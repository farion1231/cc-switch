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
        CLAUDE_CODE_SUBAGENT_MODEL: "subagent-model[1M]",
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
    expect(result.current.subagentModel).toBe("subagent-model[1M]");
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

  it("writes and clears the Claude Code subagent model env field", () => {
    let latestConfig = JSON.stringify({
      env: {
        ANTHROPIC_MODEL: "fallback-model",
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
        "CLAUDE_CODE_SUBAGENT_MODEL",
        "subagent-model[1M]",
      );
    });

    let env = JSON.parse(latestConfig).env;
    expect(env.ANTHROPIC_MODEL).toBe("fallback-model");
    expect(env.CLAUDE_CODE_SUBAGENT_MODEL).toBe("subagent-model[1M]");

    act(() => {
      result.current.handleModelChange("CLAUDE_CODE_SUBAGENT_MODEL", "");
    });

    env = JSON.parse(latestConfig).env;
    expect(env.CLAUDE_CODE_SUBAGENT_MODEL).toBeUndefined();
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

  describe("handleBatchModelChange", () => {
    it("批量更新多个模型字段，只触发一次 onConfigChange", () => {
      let latestConfig = JSON.stringify({
        env: {
          ANTHROPIC_MODEL: "old-fallback",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "old-sonnet",
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

      // 先清空调用记录（初始化可能触发过）
      onConfigChange.mockClear();

      act(() => {
        result.current.handleBatchModelChange([
          ["ANTHROPIC_MODEL", "new-fallback"],
          ["ANTHROPIC_DEFAULT_SONNET_MODEL", "new-sonnet[1M]"],
          ["ANTHROPIC_DEFAULT_SONNET_MODEL_NAME", "New Sonnet"],
        ]);
      });

      // 只触发一次 onConfigChange（而非逐字段的 3 次）
      expect(onConfigChange).toHaveBeenCalledTimes(1);

      const env = JSON.parse(latestConfig).env;
      expect(env.ANTHROPIC_MODEL).toBe("new-fallback");
      expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("new-sonnet[1M]");
      expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL_NAME).toBe("New Sonnet");
    });

    it("批量更新时空值字段从配置中删除", () => {
      let latestConfig = JSON.stringify({
        env: {
          ANTHROPIC_MODEL: "fallback",
          ANTHROPIC_DEFAULT_SONNET_MODEL: "sonnet",
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

      onConfigChange.mockClear();

      act(() => {
        result.current.handleBatchModelChange([
          ["ANTHROPIC_DEFAULT_SONNET_MODEL", ""],
        ]);
      });

      const env = JSON.parse(latestConfig).env;
      expect(env.ANTHROPIC_MODEL).toBe("fallback");
      expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBeUndefined();
    });

    it("空数组不触发 onConfigChange", () => {
      const onConfigChange = vi.fn();
      const { result } = renderHook(() =>
        useModelState({
          settingsConfig: JSON.stringify({ env: { ANTHROPIC_MODEL: "m" } }),
          onConfigChange,
        }),
      );

      onConfigChange.mockClear();

      act(() => {
        result.current.handleBatchModelChange([]);
      });

      expect(onConfigChange).not.toHaveBeenCalled();
    });

    it("批量更新删除旧键 ANTHROPIC_SMALL_FAST_MODEL", () => {
      let latestConfig = JSON.stringify({
        env: {
          ANTHROPIC_SMALL_FAST_MODEL: "legacy-small",
          ANTHROPIC_MODEL: "fallback",
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

      onConfigChange.mockClear();

      act(() => {
        result.current.handleBatchModelChange([
          ["ANTHROPIC_DEFAULT_HAIKU_MODEL", "new-haiku"],
        ]);
      });

      const env = JSON.parse(latestConfig).env;
      expect(env.ANTHROPIC_SMALL_FAST_MODEL).toBeUndefined();
      expect(env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe("new-haiku");
    });
  });

  it("不因 settingsConfig 无变化而触发冗余 setState", () => {
    const config = JSON.stringify({
      env: {
        ANTHROPIC_MODEL: "stable-model",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "stable-sonnet",
      },
    });

    // React 的 setState 若返回同一值不会触发重渲染，
    // 这里验证 hook 在相同 settingsConfig 传入时不会抛错
    const { result, rerender } = renderHook(
      ({ cfg }: { cfg: string }) =>
        useModelState({
          settingsConfig: cfg,
          onConfigChange: vi.fn(),
        }),
      { initialProps: { cfg: config } },
    );

    expect(result.current.claudeModel).toBe("stable-model");

    // 用同一 config 重新渲染，不应报错
    rerender({ cfg: config });
    expect(result.current.claudeModel).toBe("stable-model");
  });
});
