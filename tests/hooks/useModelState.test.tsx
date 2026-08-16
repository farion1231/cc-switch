import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  parseModelSuffix,
  stripClaudeOneMMarker,
  readContextWindows,
  writeContextWindow,
  stripModelSuffix,
  migrateLegacyModelSuffixes,
  useModelState,
} from "@/components/providers/forms/hooks/useModelState";

const INVALID_WINDOW_INPUTS = [
  "1e3",
  "1foo2",
  "12abc34",
  "1.5M",
  "1,000,000",
  "1_000_000",
  "1 000 000",
  "0",
  "1G",
];
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

  it("writes Claude Code subagent model env raw without frontend stripping", () => {
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
    const cleared = JSON.parse(latestConfig);
    expect(cleared.contextWindows.CLAUDE_CODE_SUBAGENT_MODEL).toBeUndefined();
  });

  it("strips Claude Code 1M markers", () => {
    expect(stripClaudeOneMMarker("deepseek-v4-pro [1M]  ")).toBe(
      "deepseek-v4-pro",
    );
    expect(stripClaudeOneMMarker("deepseek-v4-pro")).toBe("deepseek-v4-pro");
  });

  it("handleModelChange 不以后缀覆盖已配置的 contextWindows", () => {
    let latestConfig = JSON.stringify({
      env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.2[200k]" },
      contextWindows: { ANTHROPIC_DEFAULT_SONNET_MODEL: 300000 },
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
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "glm-5.3",
      );
    });

    const parsed = JSON.parse(latestConfig);
    expect(parsed.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("glm-5.3");
    // 用户显式配置的窗口保留，旧后缀只用于缺失键迁移
    expect(parsed.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(300000);
  });

  it("handleModelChange 将旧模型后缀迁移到 contextWindows", () => {
    let latestConfig = JSON.stringify({
      env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.2[200k]" },
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
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "glm-5.3",
      );
    });

    const parsed = JSON.parse(latestConfig);
    expect(parsed.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("glm-5.3");
    expect(parsed.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(200000);
  });

  it("handleModelChange 保留新输入后缀原样并迁移到 contextWindows", () => {
    let latestConfig = JSON.stringify({
      env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.2" },
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
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "glm-5.3[1M]",
      );
    });

    const parsed = JSON.parse(latestConfig);
    expect(parsed.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("glm-5.3[1M]");
    expect(parsed.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(1000000);
  });

  it("清空带后缀模型时同步删除 contextWindows", () => {
    let latestConfig = JSON.stringify({
      env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.2[200k]" },
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
      result.current.handleModelChange("ANTHROPIC_DEFAULT_SONNET_MODEL", "");
    });

    const parsed = JSON.parse(latestConfig);
    expect(parsed.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBeUndefined();
    expect(
      parsed.contextWindows?.ANTHROPIC_DEFAULT_SONNET_MODEL,
    ).toBeUndefined();
  });

  it("handleModelChange 输入 deepseek[200k] 后 model state 保留原样", () => {
    let latestConfig = JSON.stringify({
      env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.2" },
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
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "deepseek[200k]",
      );
    });

    // 前端不再硬剥后缀：state 与 env 都保留原样，迁移交给保存路径。
    expect(result.current.defaultSonnetModel).toBe("deepseek[200k]");
    const parsed = JSON.parse(latestConfig);
    expect(parsed.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("deepseek[200k]");
    expect(parsed.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(200000);
  });
});
describe("parseModelSuffix", () => {
  it.each([
    ["deepseek-v4-pro[1m]", "deepseek-v4-pro", 1000000],
    ["glm-5.2[200k]", "glm-5.2", 200000],
    ["model[500K]", "model", 500000],
    ["model[1000000]", "model", 1000000],
    ["model[1m]", "model", 1000000],
    ["model[128k]", "model", 128000],
  ] as const)("parses %s to %s/%d", (input, slug, window) => {
    expect(parseModelSuffix(input)).toEqual({ slug, window });
  });

  it.each([
    ["model", "model"],
    ["model[invalid]", "model[invalid]"],
    ["model[ 200k ]", "model[ 200k ]"],
    ["model[200 k]", "model[200 k]"],
    ["model[0]", "model[0]"],
    ["model[1.5m]", "model[1.5m]"],
  ] as const)("keeps %s unchanged without a window", (input, slug) => {
    expect(parseModelSuffix(input)).toEqual({ slug, window: undefined });
  });

  it.each(INVALID_WINDOW_INPUTS)("rejects [%s] as invalid suffix", (input) => {
    expect(parseModelSuffix(`model[${input}]`)).toEqual({
      slug: `model[${input}]`,
      window: undefined,
    });
  });
});
describe("stripModelSuffix", () => {
  it("strips [200k]", () => {
    expect(stripModelSuffix("model[200k]")).toBe("model");
  });
});
describe("contextWindows", () => {
  it("reads contextWindows from settings config", () => {
    const config = JSON.stringify({
      env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.2" },
      contextWindows: { ANTHROPIC_DEFAULT_SONNET_MODEL: 200000 },
    });

    expect(readContextWindows(config)).toEqual({
      ANTHROPIC_DEFAULT_SONNET_MODEL: 200000,
    });
  });

  it("returns empty object for missing or invalid config", () => {
    expect(readContextWindows("")).toEqual({});
    expect(readContextWindows("{invalid")).toEqual({});
    expect(readContextWindows(JSON.stringify({ env: {} }))).toEqual({});
  });

  it("returns empty object for non-object contextWindows", () => {
    expect(readContextWindows(JSON.stringify({ contextWindows: [] }))).toEqual(
      {},
    );
    expect(
      readContextWindows(JSON.stringify({ contextWindows: "bad" })),
    ).toEqual({});
  });

  it("filters non-finite and non-positive contextWindows values", () => {
    const config = JSON.stringify({
      contextWindows: {
        ANTHROPIC_MODEL: 200000,
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 0,
        ANTHROPIC_DEFAULT_SONNET_MODEL: -1,
        ANTHROPIC_DEFAULT_OPUS_MODEL: Number.POSITIVE_INFINITY,
        ANTHROPIC_DEFAULT_FABLE_MODEL: Number.NaN,
        CLAUDE_CODE_SUBAGENT_MODEL: "1000000",
        ANTHROPIC_DEFAULT_FABLE_MODEL_NAME: null,
      },
    });

    expect(readContextWindows(config)).toEqual({ ANTHROPIC_MODEL: 200000 });
  });

  it("窗口写入 contextWindows 且模型名不带后缀", () => {
    const config = JSON.stringify({
      env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.2" },
    });
    const next = writeContextWindow(
      config,
      "ANTHROPIC_DEFAULT_SONNET_MODEL",
      200000,
    );
    const parsed = JSON.parse(next);
    expect(parsed.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("glm-5.2");
    expect(parsed.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(200000);
  });

  it("写入窗口时剥离 env 中的旧后缀", () => {
    const config = JSON.stringify({
      env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "model[1M]" },
    });
    const next = writeContextWindow(
      config,
      "ANTHROPIC_DEFAULT_SONNET_MODEL",
      200000,
    );
    const parsed = JSON.parse(next);
    expect(parsed.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("model");
    expect(parsed.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(200000);
  });

  it("清空窗口时不写 contextWindows 也不写后缀", () => {
    const config = JSON.stringify({
      env: { ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.2[1M]" },
      contextWindows: { ANTHROPIC_DEFAULT_SONNET_MODEL: 1000000 },
    });
    const next = writeContextWindow(
      config,
      "ANTHROPIC_DEFAULT_SONNET_MODEL",
      null,
    );
    const parsed = JSON.parse(next);
    expect(
      parsed.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL,
    ).toBeUndefined();
    expect(parsed.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("glm-5.2");
  });

  it("writeContextWindow 对无效 JSON 不抛错并返回原 config", () => {
    const invalid = "{invalid";
    expect(() =>
      writeContextWindow(invalid, "ANTHROPIC_DEFAULT_SONNET_MODEL", 200000),
    ).not.toThrow();
    expect(
      writeContextWindow(invalid, "ANTHROPIC_DEFAULT_SONNET_MODEL", 200000),
    ).toBe(invalid);
  });

  it("writeContextWindow 会替换非对象 contextWindows", () => {
    const next = writeContextWindow(
      JSON.stringify({ contextWindows: [] }),
      "ANTHROPIC_DEFAULT_SONNET_MODEL",
      200000,
    );
    const parsed = JSON.parse(next);
    expect(Array.isArray(parsed.contextWindows)).toBe(false);
    expect(parsed.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(200000);
  });
});

describe("migrateLegacyModelSuffixes", () => {
  it("把合法旧后缀迁移进 contextWindows 并清理模型名", () => {
    const config = JSON.stringify({
      env: {
        ANTHROPIC_MODEL: "deepseek[200k]",
        ANTHROPIC_DEFAULT_SONNET_MODEL: "glm-5.2",
        ANTHROPIC_DEFAULT_OPUS_MODEL: "kimi[1M]",
      },
    });
    const next = migrateLegacyModelSuffixes(config);
    const parsed = JSON.parse(next);
    expect(parsed.env.ANTHROPIC_MODEL).toBe("deepseek");
    expect(parsed.env.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe("kimi");
    expect(parsed.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("glm-5.2");
    expect(parsed.contextWindows.ANTHROPIC_MODEL).toBe(200000);
    expect(parsed.contextWindows.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe(1000000);
    expect(
      parsed.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL,
    ).toBeUndefined();
  });

  it("已有 contextWindows 时保留用户值，不以后缀覆盖", () => {
    // 与 Rust migrate_legacy_suffix_to_context_windows 一致：只填充缺失键，
    // 已存在的 contextWindows 是用户显式配置，不能被子遗留后缀覆盖。
    const config = JSON.stringify({
      env: { ANTHROPIC_MODEL: "deepseek[200k]" },
      contextWindows: {
        ANTHROPIC_DEFAULT_SONNET_MODEL: 160000,
        ANTHROPIC_MODEL: 999,
      },
    });
    const parsed = JSON.parse(migrateLegacyModelSuffixes(config));
    expect(parsed.env.ANTHROPIC_MODEL).toBe("deepseek");
    expect(parsed.contextWindows.ANTHROPIC_MODEL).toBe(999);
    expect(parsed.contextWindows.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe(160000);
  });

  it("无合法后缀时返回原 config", () => {
    const config = JSON.stringify({
      env: { ANTHROPIC_MODEL: "deepseek-v3" },
    });
    expect(migrateLegacyModelSuffixes(config)).toBe(config);
  });

  it("非法后缀不迁移", () => {
    const config = JSON.stringify({
      env: { ANTHROPIC_MODEL: "deepseek[1.5m]" },
    });
    expect(migrateLegacyModelSuffixes(config)).toBe(config);
  });

  it("contextWindows 形状非法时整体跳过迁移", () => {
    // 与 Rust migrate_legacy_suffix_to_context_windows 一致：非对象 contextWindows
    // 不改写 env（后缀不被剥），避免把用户数据悄悄重置为空对象。
    const config = JSON.stringify({
      env: { ANTHROPIC_MODEL: "deepseek[200k]" },
      contextWindows: [],
    });
    expect(migrateLegacyModelSuffixes(config)).toBe(config);

    const configBad = JSON.stringify({
      env: { ANTHROPIC_MODEL: "deepseek[200k]" },
      contextWindows: "bad",
    });
    expect(migrateLegacyModelSuffixes(configBad)).toBe(configBad);
  });

  it("无效 JSON 不抛错并返回原 config", () => {
    const invalid = "{invalid";
    expect(() => migrateLegacyModelSuffixes(invalid)).not.toThrow();
    expect(migrateLegacyModelSuffixes(invalid)).toBe(invalid);
  });
});
