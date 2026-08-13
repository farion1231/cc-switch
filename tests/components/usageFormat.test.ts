import { describe, expect, it } from "vitest";
import {
  SPEED_ESTIMATE_MIN_OUTPUT_TOKENS,
  SPEED_MIN_OUTPUT_TOKENS,
  computeFloatingUsageSummary,
  formatEstimatedTokensPerSecond,
  formatOutputTokensPerSecond,
  formatTokensCompact,
  formatTokensPerSecond,
  formatTokensShort,
  getAggregateTokensPerSecond,
  getEstimatedTokensPerSecond,
  getOutputTokensPerSecond,
  getLocaleFromLanguage,
  sumSpeedTotals,
} from "@/components/usage/format";

describe("usage format helpers", () => {
  it("formats Traditional Chinese token units with Traditional characters", () => {
    expect(formatTokensShort(12_345, "zh-TW")).toBe("1.2 萬");
    expect(formatTokensShort(123_456_789, "zh-Hant", 2)).toBe("1.23 億");
  });

  it("resolves Traditional Chinese locale aliases", () => {
    expect(getLocaleFromLanguage("zh_TW")).toBe("zh-TW");
    expect(getLocaleFromLanguage("zh-HK")).toBe("zh-TW");
  });

  it("calculates speed from generation time after the first token", () => {
    expect(
      getOutputTokensPerSecond({
        outputTokens: 120,
        latencyMs: 10_000,
        firstTokenMs: 4_000,
      }),
    ).toBe(20);
  });

  it("ignores durationMs: speed is always output / (latency - first token)", () => {
    expect(
      getOutputTokensPerSecond({
        outputTokens: 120,
        latencyMs: 10_000,
        firstTokenMs: 4_000,
        durationMs: 3_000,
      } as Parameters<typeof getOutputTokensPerSecond>[0]),
    ).toBe(20);
  });

  it("does not compute speed without first-token timing (session logs, non-streaming)", () => {
    expect(
      getOutputTokensPerSecond({ outputTokens: 1_200, latencyMs: 10_000 }),
    ).toBeNull();
    expect(
      getOutputTokensPerSecond({
        outputTokens: 1_200,
        latencyMs: undefined,
        firstTokenMs: 1_000,
      }),
    ).toBeNull();
  });

  it("skips requests with fewer than 100 output tokens", () => {
    expect(SPEED_MIN_OUTPUT_TOKENS).toBe(100);
    expect(
      getOutputTokensPerSecond({
        outputTokens: 99,
        latencyMs: 1_000,
        firstTokenMs: 900,
      }),
    ).toBeNull();
    expect(
      getOutputTokensPerSecond({
        outputTokens: 100,
        latencyMs: 2_000,
        firstTokenMs: 1_000,
      }),
    ).toBe(100);
  });

  it("does not show speed when the generation time is not positive", () => {
    expect(
      formatOutputTokensPerSecond({
        outputTokens: 0,
        latencyMs: 10_000,
        firstTokenMs: 1_000,
      }),
    ).toBeNull();
    expect(
      formatOutputTokensPerSecond({
        outputTokens: 120,
        latencyMs: 4_000,
        firstTokenMs: 4_000,
      }),
    ).toBeNull();
    expect(
      formatOutputTokensPerSecond({
        outputTokens: 120,
        latencyMs: 3_000,
        firstTokenMs: 4_000,
      }),
    ).toBeNull();
  });

  it("hides speed when the generation window is a transport burst", () => {
    // 2842 tps 实例：输出在首字后 19ms 内全部到达，算出来的是传输突发
    expect(
      getOutputTokensPerSecond({
        outputTokens: 154,
        latencyMs: 3_444,
        firstTokenMs: 3_425,
      }),
    ).toBeNull();
    expect(
      getOutputTokensPerSecond({
        outputTokens: 150,
        latencyMs: 1_100,
        firstTokenMs: 1_000,
      }),
    ).toBe(1500);
  });

  it("formats speed with integer or single-decimal precision", () => {
    expect(
      formatOutputTokensPerSecond({
        outputTokens: 121,
        latencyMs: 11_000,
        firstTokenMs: 1_000,
      }),
    ).toBe("12");
    expect(formatTokensPerSecond(0.25)).toBe("0.3");
    expect(formatTokensPerSecond(null)).toBeNull();
  });

  it("estimates speed for session-log requests from the whole duration", () => {
    expect(SPEED_ESTIMATE_MIN_OUTPUT_TOKENS).toBe(200);
    const session = {
      outputTokens: 1_800,
      latencyMs: 20_000,
      dataSource: "session_log",
    };
    // 1800 token / 20 s，等首字的时间也算在内
    expect(getEstimatedTokensPerSecond(session)).toBe(90);
    expect(formatEstimatedTokensPerSecond(session)).toBe("90");
    // 估算的不算精确速度
    expect(getOutputTokensPerSecond(session)).toBeNull();
    expect(
      getEstimatedTokensPerSecond({ ...session, dataSource: "codex_session" }),
    ).toBe(90);
  });

  it("does not estimate speed for routed, short, or untimed requests", () => {
    const session = {
      outputTokens: 1_800,
      latencyMs: 20_000,
      dataSource: "session_log",
    };
    // 路由服务的请求：非流式没有首字，也不估
    expect(
      getEstimatedTokensPerSecond({ ...session, dataSource: "proxy" }),
    ).toBeNull();
    expect(
      getEstimatedTokensPerSecond({ ...session, dataSource: undefined }),
    ).toBeNull();
    // 输出不到 200
    expect(
      getEstimatedTokensPerSecond({ ...session, outputTokens: 199 }),
    ).toBeNull();
    // 没估出耗时
    expect(
      getEstimatedTokensPerSecond({ ...session, latencyMs: 0 }),
    ).toBeNull();
    // 耗时不到 1 秒
    expect(
      getEstimatedTokensPerSecond({ ...session, latencyMs: 999 }),
    ).toBeNull();
  });

  it("aggregates speed as total output over total generation time, not a mean of rates", () => {
    const logs = [
      // 1000 token / 10 s = 100 tok/s
      { outputTokens: 1_000, latencyMs: 11_000, firstTokenMs: 1_000 },
      // 200 token / 0.5 s = 400 tok/s（逐条平均会被它拉高）
      { outputTokens: 200, latencyMs: 1_500, firstTokenMs: 1_000 },
      // 不计：输出不到 100
      { outputTokens: 50, latencyMs: 1_100, firstTokenMs: 1_000 },
      // 不计：没有首字
      { outputTokens: 5_000, latencyMs: 9_000 },
    ];
    const totals = sumSpeedTotals(logs);
    expect(totals).toEqual({ outputTokens: 1_200, generationMs: 10_500 });
    const tps = getAggregateTokensPerSecond(
      totals.outputTokens,
      totals.generationMs,
    );
    expect(tps).toBeCloseTo(114.29, 2);
    expect(getAggregateTokensPerSecond(0, 0)).toBeNull();
    expect(getAggregateTokensPerSecond(500, 0)).toBeNull();
    expect(getAggregateTokensPerSecond(undefined, 1_000)).toBeNull();
  });

  it("formats compact token counts with K/M/B and three significant digits", () => {
    expect(formatTokensCompact(9_999, "en-US")).toBe("9,999");
    expect(formatTokensCompact(48_210, "en-US")).toBe("48.2K");
    expect(formatTokensCompact(189_400, "en-US")).toBe("189K");
    expect(formatTokensCompact(999_960, "en-US")).toBe("1M");
    expect(formatTokensCompact(18_200_000, "en-US")).toBe("18.2M");
    expect(formatTokensCompact(2_340_000_000, "en-US")).toBe("2.34B");
    expect(formatTokensCompact("x")).toBe("--");
  });
});

describe("computeFloatingUsageSummary", () => {
  it("sums cache tokens into realTotalTokens and computes hit rate", () => {
    const summary = computeFloatingUsageSummary([
      {
        appType: "claude",
        summary: {
          totalRequests: 2,
          totalCost: "0.30",
          totalInputTokens: 100,
          totalOutputTokens: 50,
          totalCacheCreationTokens: 20,
          totalCacheReadTokens: 30,
          successRate: 100,
          realTotalTokens: 200,
          cacheHitRate: 0.2,
        },
      },
    ]);

    expect(summary.realTotalTokens).toBe(200); // 100 + 50 + 20 + 30
    expect(summary.inputTokens).toBe(100);
    expect(summary.outputTokens).toBe(50);
    expect(summary.cacheCreationTokens).toBe(20);
    expect(summary.cacheReadTokens).toBe(30);
    // 30 / (100 + 20 + 30) = 20%
    expect(summary.cacheHitRate).toBeCloseTo(20, 5);
    expect(summary.totalCost).toBeCloseTo(0.3, 5);
  });

  it("aggregates across multiple app summaries", () => {
    const summary = computeFloatingUsageSummary([
      {
        appType: "claude",
        summary: {
          totalRequests: 1,
          totalCost: "0.10",
          totalInputTokens: 100,
          totalOutputTokens: 50,
          totalCacheCreationTokens: 0,
          totalCacheReadTokens: 0,
          successRate: 100,
          realTotalTokens: 150,
          cacheHitRate: 0,
        },
      },
      {
        appType: "codex",
        summary: {
          totalRequests: 1,
          totalCost: "0.20",
          totalInputTokens: 200,
          totalOutputTokens: 30,
          totalCacheCreationTokens: 0,
          totalCacheReadTokens: 70,
          successRate: 100,
          realTotalTokens: 300,
          cacheHitRate: 0.7,
        },
      },
    ]);

    expect(summary.realTotalTokens).toBe(450); // (100+50) + (200+30+70)
    expect(summary.totalCost).toBeCloseTo(0.3, 5);
    // input 跨应用累计 = 300，cacheableInput = 300 + 70；70 / 370 = 18.9...
    expect(summary.cacheHitRate).toBeCloseTo(18.9189, 3);
  });

  it("returns zeros for empty data", () => {
    expect(computeFloatingUsageSummary([])).toEqual({
      totalCost: 0,
      realTotalTokens: 0,
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      cacheHitRate: 0,
    });
    expect(computeFloatingUsageSummary(undefined)).toEqual({
      totalCost: 0,
      realTotalTokens: 0,
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      cacheHitRate: 0,
    });
  });
});
