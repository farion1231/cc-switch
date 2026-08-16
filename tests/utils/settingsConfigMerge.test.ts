import { describe, expect, it } from "vitest";
import {
  applyAutoSyncCompactRatioSetting,
  applyAutoSyncContextWindowSetting,
  mergeSettingsConfigPreservingAutoSync,
  resolveAutoSyncContextWindow,
  matchesAutoSyncSource,
  restoreAutoSyncContextWindowValue,
} from "@/utils/settingsConfigMerge";

describe("mergeSettingsConfigPreservingAutoSync", () => {
  it("保留当前 false，即使新配置基于旧快照重建并缺少该字段", () => {
    const current = JSON.stringify({
      env: { ANTHROPIC_MODEL: "model-a" },
      autoSyncContextWindow: false,
    });
    const staleNext = JSON.stringify({
      env: { ANTHROPIC_MODEL: "model-b" },
    });

    const merged = mergeSettingsConfigPreservingAutoSync(current, staleNext);
    expect(JSON.parse(merged)).toEqual({
      env: { ANTHROPIC_MODEL: "model-b" },
      autoSyncContextWindow: false,
    });
  });

  it("覆盖新配置里的旧 true，避免其他参数更新把开关重新打开", () => {
    const current = JSON.stringify({
      env: {},
      autoSyncContextWindow: false,
    });
    const staleNext = JSON.stringify({
      env: { ANTHROPIC_BASE_URL: "https://example.com" },
      autoSyncContextWindow: true,
    });

    const merged = mergeSettingsConfigPreservingAutoSync(current, staleNext);
    expect(JSON.parse(merged).autoSyncContextWindow).toBe(false);
  });

  it("当前没有显式开关时不额外写入字段", () => {
    const current = JSON.stringify({ env: {} });
    const next = JSON.stringify({ env: { ANTHROPIC_MODEL: "model" } });

    const merged = mergeSettingsConfigPreservingAutoSync(current, next);
    expect(JSON.parse(merged)).toEqual({
      env: { ANTHROPIC_MODEL: "model" },
    });
  });

  it("解析失败时原样返回新配置，不打断表单编辑", () => {
    expect(
      mergeSettingsConfigPreservingAutoSync("{bad json", "{also bad"),
    ).toBe("{also bad");
  });

  it("关闭自动同步时清理 ACW/MAX 并写入 false", () => {
    const updated = applyAutoSyncContextWindowSetting(
      JSON.stringify({
        env: {
          ANTHROPIC_MODEL: "model[1M]",
          CLAUDE_CODE_AUTO_COMPACT_WINDOW: "800000",
          CLAUDE_CODE_MAX_CONTEXT_TOKENS: "1000000",
        },
      }),
      false,
    );

    expect(JSON.parse(updated)).toEqual({
      env: { ANTHROPIC_MODEL: "model[1M]" },
      autoSyncContextWindow: false,
    });
  });

  it("重新开启自动同步时无账本不写回 ACW/MAX", () => {
    const updated = applyAutoSyncContextWindowSetting(
      JSON.stringify({ env: { ANTHROPIC_MODEL: "model[1M]" } }),
      true,
    );

    expect(JSON.parse(updated)).toEqual({
      env: { ANTHROPIC_MODEL: "model[1M]" },
      autoSyncContextWindow: true,
    });
  });

  it("重新开启自动同步时忽略 userExplicit，从 lastWritten/staticInjected 恢复", () => {
    const updated = applyAutoSyncContextWindowSetting(
      JSON.stringify({
        env: { ANTHROPIC_MODEL: "model[1M]" },
        autoSyncState: {
          userExplicit: { ACW: "160000", MAX: "200000" },
          lastWritten: { ACW: "170000" },
          staticInjected: { MAX: "262144" },
        },
      }),
      true,
    );

    expect(JSON.parse(updated)).toEqual({
      env: {
        ANTHROPIC_MODEL: "model[1M]",
        CLAUDE_CODE_AUTO_COMPACT_WINDOW: "170000",
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "262144",
      },
      autoSyncContextWindow: true,
      autoSyncState: {
        userExplicit: { ACW: "160000", MAX: "200000" },
        lastWritten: { ACW: "170000" },
        staticInjected: { MAX: "262144" },
      },
    });
  });

  it("重新开启自动同步时无显式值从 lastWritten/staticInjected 恢复", () => {
    const updated = applyAutoSyncContextWindowSetting(
      JSON.stringify({
        env: { ANTHROPIC_MODEL: "model[1M]" },
        autoSyncState: {
          lastWritten: { ACW: "160000" },
          staticInjected: { MAX: "262144" },
        },
      }),
      true,
    );

    expect(JSON.parse(updated)).toEqual({
      env: {
        ANTHROPIC_MODEL: "model[1M]",
        CLAUDE_CODE_AUTO_COMPACT_WINDOW: "160000",
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "262144",
      },
      autoSyncContextWindow: true,
      autoSyncState: {
        lastWritten: { ACW: "160000" },
        staticInjected: { MAX: "262144" },
      },
    });
  });


  it("当前开关为 false 时，其他参数更新保留手动 ACW/MAX", () => {
    const current = JSON.stringify({ env: {}, autoSyncContextWindow: false });
    const staleNext = JSON.stringify({
      env: {
        ANTHROPIC_BASE_URL: "https://example.com",
        CLAUDE_CODE_AUTO_COMPACT_WINDOW: "800000",
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "1000000",
      },
    });

    const merged = mergeSettingsConfigPreservingAutoSync(current, staleNext);
    expect(JSON.parse(merged)).toEqual({
      env: {
        ANTHROPIC_BASE_URL: "https://example.com",
        CLAUDE_CODE_AUTO_COMPACT_WINDOW: "800000",
        CLAUDE_CODE_MAX_CONTEXT_TOKENS: "1000000",
      },
      autoSyncContextWindow: false,
    });
  });

  it("当前缺失开关时，自动写入不得把旧 true 补写回来", () => {
    const current = JSON.stringify({ env: {} });
    const staleNext = JSON.stringify({
      env: { ANTHROPIC_MODEL: "model" },
      autoSyncContextWindow: true,
    });

    const merged = mergeSettingsConfigPreservingAutoSync(current, staleNext);
    expect(JSON.parse(merged)).toEqual({ env: { ANTHROPIC_MODEL: "model" } });
  });

  it("其他参数更新时保留当前压缩比例", () => {
    const current = JSON.stringify({
      env: { ANTHROPIC_MODEL: "model-a" },
      autoSyncContextWindow: true,
      autoSyncCompactRatio: 0.7,
    });
    const staleNext = JSON.stringify({
      env: { ANTHROPIC_MODEL: "model-b" },
    });

    const merged = mergeSettingsConfigPreservingAutoSync(current, staleNext);
    expect(JSON.parse(merged)).toEqual({
      env: { ANTHROPIC_MODEL: "model-b" },
      autoSyncContextWindow: true,
      autoSyncCompactRatio: 0.7,
    });
  });

  it("当前压缩比例超过 0.95 时不保留", () => {
    const current = JSON.stringify({
      env: { ANTHROPIC_MODEL: "model-a" },
      autoSyncContextWindow: true,
      autoSyncCompactRatio: 0.96,
    });
    const staleNext = JSON.stringify({
      env: { ANTHROPIC_MODEL: "model-b" },
      autoSyncCompactRatio: 0.8,
    });

    const merged = mergeSettingsConfigPreservingAutoSync(current, staleNext);
    expect(JSON.parse(merged)).toEqual({
      env: { ANTHROPIC_MODEL: "model-b" },
      autoSyncContextWindow: true,
    });
  });

  it("当前压缩比例 0.2 时保留", () => {
    const current = JSON.stringify({
      env: {},
      autoSyncContextWindow: true,
      autoSyncCompactRatio: 0.2,
    });
    const staleNext = JSON.stringify({ env: { ANTHROPIC_MODEL: "model" } });

    const merged = mergeSettingsConfigPreservingAutoSync(current, staleNext);
    expect(JSON.parse(merged)).toEqual({
      env: { ANTHROPIC_MODEL: "model" },
      autoSyncContextWindow: true,
      autoSyncCompactRatio: 0.2,
    });
  });

  it("当前压缩比例低于 0.2 时不保留", () => {
    const current = JSON.stringify({
      env: {},
      autoSyncContextWindow: true,
      autoSyncCompactRatio: 0.19,
    });
    const staleNext = JSON.stringify({
      env: { ANTHROPIC_MODEL: "model" },
      autoSyncCompactRatio: 0.8,
    });

    const merged = mergeSettingsConfigPreservingAutoSync(current, staleNext);
    expect(JSON.parse(merged)).toEqual({
      env: { ANTHROPIC_MODEL: "model" },
      autoSyncContextWindow: true,
    });
  });

  it("当前没有显式比例时不额外写入字段", () => {
    const current = JSON.stringify({ env: {}, autoSyncContextWindow: true });
    const next = JSON.stringify({
      env: { ANTHROPIC_MODEL: "model" },
      autoSyncCompactRatio: 0.5,
    });

    const merged = mergeSettingsConfigPreservingAutoSync(current, next);
    expect(JSON.parse(merged).autoSyncCompactRatio).toBeUndefined();
  });

  it("写入压缩比例时会保留 autoSyncContextWindow", () => {
    const updated = applyAutoSyncCompactRatioSetting(
      JSON.stringify({ env: {}, autoSyncContextWindow: true }),
      0.6,
    );

    expect(JSON.parse(updated)).toEqual({
      env: {},
      autoSyncContextWindow: true,
      autoSyncCompactRatio: 0.6,
    });
  });

  it("清空压缩比例时删除字段并保留其他配置", () => {
    const updated = applyAutoSyncCompactRatioSetting(
      JSON.stringify({
        env: { ANTHROPIC_MODEL: "model" },
        autoSyncContextWindow: true,
        autoSyncCompactRatio: 0.8,
      }),
      null,
    );

    expect(JSON.parse(updated)).toEqual({
      env: { ANTHROPIC_MODEL: "model" },
      autoSyncContextWindow: true,
    });
  });

  it("关闭时按 lastWritten/staticInjected 短键删除 ACW/MAX 并保留账本", () => {
    const updated = applyAutoSyncContextWindowSetting(
      JSON.stringify({
        env: {
          CLAUDE_CODE_AUTO_COMPACT_WINDOW: "160000",
          CLAUDE_CODE_MAX_CONTEXT_TOKENS: "262144",
        },
        autoSyncContextWindow: true,
        autoSyncState: {
          lastWritten: { ACW: "160000" },
          staticInjected: { MAX: "262144" },
          userExplicit: {},
        },
      }),
      false,
    );

    const parsed = JSON.parse(updated);
    expect(parsed.env.CLAUDE_CODE_AUTO_COMPACT_WINDOW).toBeUndefined();
    expect(parsed.env.CLAUDE_CODE_MAX_CONTEXT_TOKENS).toBeUndefined();
    expect(parsed.autoSyncState.lastWritten).toEqual({ ACW: "160000" });
    expect(parsed.autoSyncState.staticInjected).toEqual({ MAX: "262144" });
  });

  it("关闭瞬间 live 中的非自动值保留且不写 userExplicit", () => {
    const updated = applyAutoSyncContextWindowSetting(
      JSON.stringify({
        env: {
          CLAUDE_CODE_AUTO_COMPACT_WINDOW: "250000",
          CLAUDE_CODE_MAX_CONTEXT_TOKENS: "250000",
        },
        autoSyncContextWindow: true,
        autoSyncState: {
          lastWritten: { ACW: "160000", MAX: "200000" },
          staticInjected: {},
        },
      }),
      false,
    );

    const parsed = JSON.parse(updated);
    expect(parsed.env.CLAUDE_CODE_AUTO_COMPACT_WINDOW).toBe("250000");
    expect(parsed.env.CLAUDE_CODE_MAX_CONTEXT_TOKENS).toBe("250000");
    expect(parsed.autoSyncState.userExplicit).toBeUndefined();
    expect(parsed.autoSyncState.lastWritten).toEqual({
      ACW: "160000",
      MAX: "200000",
    });
  });

  it("关闭时 live 值匹配 lastWritten 即删除，userExplicit 不再优先", () => {
    const updated = applyAutoSyncContextWindowSetting(
      JSON.stringify({
        env: { CLAUDE_CODE_MAX_CONTEXT_TOKENS: "200000" },
        autoSyncContextWindow: true,
        autoSyncState: {
          lastWritten: { MAX: "200000" },
          staticInjected: {},
          userExplicit: { MAX: "200000" },
        },
      }),
      false,
    );

    const parsed = JSON.parse(updated);
    expect(parsed.env.CLAUDE_CODE_MAX_CONTEXT_TOKENS).toBeUndefined();
    expect(parsed.autoSyncState.userExplicit).toEqual({ MAX: "200000" });
  });

});

describe("resolveAutoSyncContextWindow", () => {
  it("显式字段优先返回", () => {
    expect(
      resolveAutoSyncContextWindow(
        JSON.stringify({ autoSyncContextWindow: true, autoSyncState: {} }),
      ),
    ).toBe(true);
    expect(
      resolveAutoSyncContextWindow(
        JSON.stringify({
          autoSyncContextWindow: false,
          autoSyncState: { lastWritten: { ACW: "1", MAX: "2" } },
        }),
      ),
    ).toBe(false);
  });

  it("字段缺失时即使账本有记录也视为关闭", () => {
    expect(
      resolveAutoSyncContextWindow(
        JSON.stringify({
          env: {},
          autoSyncState: { lastWritten: { ACW: "190000", MAX: "200000" } },
        }),
      ),
    ).toBe(false);
    expect(
      resolveAutoSyncContextWindow(
        JSON.stringify({
          env: {},
          autoSyncState: { staticInjected: { ACW: "262144", MAX: "262144" } },
        }),
      ),
    ).toBe(false);
  });

  it("字段缺失且无账本时视为关闭", () => {
    expect(resolveAutoSyncContextWindow(JSON.stringify({ env: {} }))).toBe(false);
  });

  it("字段缺失时即使 env 命中 contextWindows 推导目标也视为关闭", () => {
    expect(
      resolveAutoSyncContextWindow(
        JSON.stringify({
          env: {
            CLAUDE_CODE_AUTO_COMPACT_WINDOW: "190000",
            CLAUDE_CODE_MAX_CONTEXT_TOKENS: "200000",
          },
          contextWindows: { ANTHROPIC_DEFAULT_SONNET_MODEL: 200000 },
        }),
      ),
    ).toBe(false);
  });

  it("无效 JSON 返回 false", () => {
    expect(resolveAutoSyncContextWindow("{invalid")).toBe(false);
  });
});

describe("ledger contract parity with Rust", () => {
  it("账本使用 ACW/MAX 短键结构", () => {
    // 关闭自动同步时按短键识别自动来源并清空 live ACW/MAX
    const updated = applyAutoSyncContextWindowSetting(
      JSON.stringify({
        env: {
          CLAUDE_CODE_AUTO_COMPACT_WINDOW: "160000",
          CLAUDE_CODE_MAX_CONTEXT_TOKENS: "200000",
        },
        autoSyncContextWindow: true,
        autoSyncState: {
          lastWritten: { ACW: "160000", MAX: "200000" },
          staticInjected: {},
          userExplicit: {},
        },
      }),
      false,
    );
    const parsed = JSON.parse(updated);
    expect(parsed.env.CLAUDE_CODE_AUTO_COMPACT_WINDOW).toBeUndefined();
    expect(parsed.env.CLAUDE_CODE_MAX_CONTEXT_TOKENS).toBeUndefined();
  });

  it("字符串 coercion：账本值为字符串时匹配 live 字符串", () => {
    expect(
      matchesAutoSyncSource({ staticInjected: { ACW: "200000" } }, "ACW", "200000"),
    ).toBe(true);
  });

  it("字符串 coercion：数字型账本值不匹配（对齐 Rust as_str）", () => {
    expect(
      matchesAutoSyncSource({ staticInjected: { ACW: 200000 } }, "ACW", "200000"),
    ).toBe(false);
  });


  it("恢复只读 lastWritten/staticInjected，忽略 userExplicit", () => {
    expect(
      restoreAutoSyncContextWindowValue(
        {
          userExplicit: { MAX: "250000" },
          lastWritten: { MAX: "200000" },
          staticInjected: { MAX: "300000" },
        },
        "MAX",
      ),
    ).toBe("200000");
    expect(
      restoreAutoSyncContextWindowValue(
        {
          userExplicit: { MAX: "250000" },
          lastWritten: {},
          staticInjected: { MAX: "300000" },
        },
        "MAX",
      ),
    ).toBe("300000");
  });
});
