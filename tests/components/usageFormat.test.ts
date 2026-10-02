import { describe, expect, it } from "vitest";
import {
  SPEED_MIN_OUTPUT_TOKENS,
  formatOutputTokensPerSecond,
  formatTokensCompact,
  formatTokensPerSecond,
  formatTokensShort,
  getAggregateTokensPerSecond,
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
