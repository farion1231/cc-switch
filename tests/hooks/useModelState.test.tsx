import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  parseModelSuffix,
  hasClaudeOneMMarker,
  setClaudeOneMMarker,
  stripClaudeOneMMarker,
  setModelSuffix,
  reapplySuffix,
  stripModelSuffix,
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
});

describe("parseModelSuffix", () => {
  it("parses [1m] suffix", () => {
    expect(parseModelSuffix("deepseek-v4-pro[1m]")).toEqual({
      slug: "deepseek-v4-pro",
      window: 1000000,
    });
  });

  it("parses [200k] suffix", () => {
    expect(parseModelSuffix("glm-5.2[200k]")).toEqual({
      slug: "glm-5.2",
      window: 200000,
    });
  });

  it("parses uppercase [500K]", () => {
    expect(parseModelSuffix("model[500K]")).toEqual({
      slug: "model",
      window: 500000,
    });
  });

  it("parses pure number [1000000]", () => {
    expect(parseModelSuffix("model[1000000]")).toEqual({
      slug: "model",
      window: 1000000,
    });
  });

  it("returns undefined window for no suffix", () => {
    expect(parseModelSuffix("model")).toEqual({
      slug: "model",
      window: undefined,
    });
  });

  it("does not strip invalid suffix", () => {
    expect(parseModelSuffix("model[invalid]")).toEqual({
      slug: "model[invalid]",
      window: undefined,
    });
  });

  it("parses lowercase [1m]", () => {
    expect(parseModelSuffix("model[1m]")).toEqual({
      slug: "model",
      window: 1000000,
    });
  });

  it("parses [128k] suffix", () => {
    expect(parseModelSuffix("model[128k]")).toEqual({
      slug: "model",
      window: 128000,
    });
  });

  it("rejects [0] as invalid window", () => {
    expect(parseModelSuffix("model[0]")).toEqual({
      slug: "model[0]",
      window: undefined,
    });
  });

  it("parses [1.5m] decimal as 1500000", () => {
    expect(parseModelSuffix("model[1.5m]")).toEqual({
      slug: "model",
      window: 1500000,
    });
  });
});

describe("setModelSuffix", () => {
  it("appends lowercase suffix", () => {
    expect(setModelSuffix("model", "1M")).toBe("model[1m]");
  });

  it("clears suffix when empty", () => {
    expect(setModelSuffix("model[1m]", "")).toBe("model");
  });

  it("replaces existing suffix", () => {
    expect(setModelSuffix("model[1m]", "200K")).toBe("model[200k]");
  });

  it("writes lowercase [200k] from 200K", () => {
    expect(setModelSuffix("model", "200K")).toBe("model[200k]");
  });

  it("writes lowercase [128k] from 128K", () => {
    expect(setModelSuffix("model", "128K")).toBe("model[128k]");
  });

  it("writes pure number [1000000]", () => {
    expect(setModelSuffix("model", "1000000")).toBe("model[1000000]");
  });

  it("returns empty string for empty base", () => {
    expect(setModelSuffix("", "1M")).toBe("");
  });

  it("returns base unchanged for invalid input abc", () => {
    expect(setModelSuffix("model", "abc")).toBe("model");
  });

  it("returns base unchanged for unsupported unit 1G", () => {
    expect(setModelSuffix("model", "1G")).toBe("model");
  });
});

describe("setModelSuffix - 多元化输入", () => {
  it("accepts input with brackets [30k]", () => {
    expect(setModelSuffix("model", "[30k]")).toBe("model[30k]");
  });

  it("accepts input with trailing bracket [30", () => {
    expect(setModelSuffix("model", "[30")).toBe("model[30]");
  });

  it("accepts input with leading bracket 30k]", () => {
    expect(setModelSuffix("model", "30k]")).toBe("model[30k]");
  });

  it("accepts comma-separated number 1,000,000", () => {
    expect(setModelSuffix("model", "1,000,000")).toBe("model[1000000]");
  });

  it("accepts underscore-separated number 1_000_000", () => {
    expect(setModelSuffix("model", "1_000_000")).toBe("model[1000000]");
  });

  it('accepts space-separated "1 000 000"', () => {
    expect(setModelSuffix("model", "1 000 000")).toBe("model[1000000]");
  });

  it("accepts decimal 1.5M as 1500000", () => {
    expect(setModelSuffix("model", "1.5M")).toBe("model[1500000]");
  });

  it("accepts decimal 0.5M as 500000", () => {
    expect(setModelSuffix("model", "0.5M")).toBe("model[500000]");
  });

  it("accepts mixed input [1,000k]", () => {
    expect(setModelSuffix("model", "[1,000k]")).toBe("model[1000k]");
  });
});

describe("stripModelSuffix", () => {
  it("strips [200k]", () => {
    expect(stripModelSuffix("model[200k]")).toBe("model");
  });
});

describe("reapplySuffix", () => {
  it("preserves suffix when changing model name", () => {
    expect(reapplySuffix("deepseek-v4-pro[200k]", "glm-5.2")).toBe(
      "glm-5.2[200k]",
    );
  });

  it("returns base unchanged when old model has no suffix", () => {
    expect(reapplySuffix("deepseek-v4-pro", "glm-5.2")).toBe("glm-5.2");
  });

  it("returns empty string when new input is empty", () => {
    expect(reapplySuffix("deepseek-v4-pro[200k]", "")).toBe("");
  });

  it("old suffix wins when new input also has a suffix", () => {
    expect(reapplySuffix("deepseek-v4-pro[200k]", "glm-5.2[100k]")).toBe(
      "glm-5.2[200k]",
    );
  });
});
