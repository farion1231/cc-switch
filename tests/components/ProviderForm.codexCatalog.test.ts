import { describe, expect, it } from "vitest";
import { normalizeCodexCatalogModelsForSave } from "@/components/providers/forms/ProviderForm";
import { mapCodexCatalogModelForForm } from "@/components/providers/forms/hooks/useCodexConfigState";

describe("ProviderForm Codex catalog helpers", () => {
  it("normalizes catalog rows and removes empty or duplicate models", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        { model: " deepseek-v4-flash ", displayName: " DeepSeek " },
        { model: "deepseek-v4-flash", displayName: "Duplicate" },
        { model: "", displayName: "Empty" },
        { model: "kimi-k2", contextWindow: "128000 tokens" },
      ]),
    ).toEqual([
      { model: "deepseek-v4-flash", displayName: "DeepSeek" },
      // 非法 token 整字段丢弃，与后端 parse_codex_positive_u64 的弃值语义一致
      { model: "kimi-k2" },
    ]);
  });

  it("normalizes context-window tokens like the backend and drops unparseable ones", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        { model: "a", contextWindow: "1M" },
        { model: "b", contextWindow: "256k" },
        { model: "c", contextWindow: "500000" },
        { model: "d", contextWindow: 1000000 },
        { model: "e", contextWindow: " 128K " },
        { model: "f", contextWindow: "0.5M" },
        { model: "g", contextWindow: "0" },
        { model: "h", contextWindow: "" },
        { model: "i" },
        // 小写 m / k 同样接受
        { model: "j", contextWindow: "2m" },
        // 超出安全整数与带符号形态：一律丢弃
        // （数组/对象等非法形态编译期即被 CodexCatalogModel 类型拦下；
        //   运行时守卫兜底未经类型校验的数据）
        { model: "k", contextWindow: "99999999999999999999" },
        { model: "l", contextWindow: "+128000" },
      ]),
    ).toEqual([
      { model: "a", contextWindow: 1000000 },
      { model: "b", contextWindow: 256000 },
      { model: "c", contextWindow: 500000 },
      { model: "d", contextWindow: 1000000 },
      { model: "e", contextWindow: 128000 },
      { model: "f" },
      { model: "g" },
      { model: "h" },
      { model: "i" },
      { model: "j", contextWindow: 2000000 },
      { model: "k" },
      { model: "l" },
    ]);
  });

  it("round-trips a hand-written string context window into a number instead of stripping it", () => {
    // 手编 JSON 的 "1M" 经加载映射保留字符串，保存侧归一化为数值——
    // 旧的 [^\d] 剥字实现会把同一回环静默剥成 1。
    const roundTripped = normalizeCodexCatalogModelsForSave(
      [{ model: "glm-5.2", context_window: "1M" }].map(
        mapCodexCatalogModelForForm,
      ),
    );

    expect(roundTripped).toEqual([
      { model: "glm-5.2", contextWindow: 1000000 },
    ]);
  });

  it("preserves native-profile overrides (parallel tool calls + input modalities + base instructions)", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        {
          model: "MiniMax-M3",
          displayName: "MiniMax-M3",
          contextWindow: 1000000,
          supportsParallelToolCalls: true,
          inputModalities: ["text", "image"],
          baseInstructions:
            "  You are Codex, a coding agent based on MiniMax-M3.  ",
        },
        // false must be preserved (not dropped as falsy); empty modalities dropped;
        // empty/whitespace baseInstructions dropped
        {
          model: "mimo-v2.5-pro",
          supportsParallelToolCalls: false,
          inputModalities: [],
          baseInstructions: "   ",
        },
      ]),
    ).toEqual([
      {
        model: "MiniMax-M3",
        displayName: "MiniMax-M3",
        contextWindow: 1000000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        baseInstructions: "You are Codex, a coding agent based on MiniMax-M3.",
      },
      { model: "mimo-v2.5-pro", supportsParallelToolCalls: false },
    ]);
  });

  it("preserves per-model reasoning levels and default level", () => {
    expect(
      normalizeCodexCatalogModelsForSave([
        {
          model: "deepseek-v4-flash",
          displayName: "DeepSeek V4 Flash",
          reasoningLevels: ["none", "low", "medium", "high", "xhigh", "max"],
          defaultReasoningLevel: " xhigh ",
        },
        // empty levels / whitespace default are dropped
        {
          model: "plain-model",
          reasoningLevels: [],
          defaultReasoningLevel: "   ",
        },
      ]),
    ).toEqual([
      {
        model: "deepseek-v4-flash",
        displayName: "DeepSeek V4 Flash",
        reasoningLevels: ["none", "low", "medium", "high", "xhigh", "max"],
        defaultReasoningLevel: "xhigh",
      },
      { model: "plain-model" },
    ]);
  });

  it("round-trips reasoning levels through load and save without loss", () => {
    // load→save 回环：加载映射（mapCodexCatalogModelForForm）与保存归一化
    // （normalizeCodexCatalogModelsForSave）各锁半边时，回环丢字段两边都测不出——
    // 而编辑保存丢表会让依赖逐模型档位的功能（zen 钳制）静默失效且 UI 无可察觉。
    const stored = [
      {
        model: "glm-5.2",
        displayName: "GLM 5.2",
        reasoningLevels: ["high", "max"],
      },
      // 手写/旧数据可能是 snake_case，加载侧兼容后保存侧同样要留住
      { model: "deepseek-v4-flash", reasoning_levels: ["low", "high", "max"] },
      { model: "glm-5.1" }, // toggle 型：无表，全程不得凭空造表
    ];

    const roundTripped = normalizeCodexCatalogModelsForSave(
      stored.map(mapCodexCatalogModelForForm),
    );

    expect(roundTripped).toEqual([
      {
        model: "glm-5.2",
        displayName: "GLM 5.2",
        reasoningLevels: ["high", "max"],
      },
      { model: "deepseek-v4-flash", reasoningLevels: ["low", "high", "max"] },
      { model: "glm-5.1" },
    ]);
  });

  it("trims reasoning level values on save", () => {
    // 手编 JSON 里的 " high " 不得原样落库/发给上游。
    expect(
      normalizeCodexCatalogModelsForSave([
        { model: "glm-5.2", reasoningLevels: [" high ", "max"] },
      ]),
    ).toEqual([{ model: "glm-5.2", reasoningLevels: ["high", "max"] }]);
  });
});
