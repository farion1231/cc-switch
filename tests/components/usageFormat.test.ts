import { describe, expect, it } from "vitest";
import {
  SPEED_ESTIMATE_MIN_OUTPUT_TOKENS,
  SPEED_MIN_OUTPUT_TOKENS,
  formatEstimatedTokensPerSecond,
  formatFirstTokenSeconds,
  formatMoney,
  formatMoneyCompact,
  formatMoneyShort,
  formatHitRatePercent,
  formatMoneyTick,
  formatOutputTokensPerSecond,
  formatTokensCompact,
  formatTokensPerSecond,
  formatTokensShort,
  resolveMoneyDisplay,
  resolveSidebarToday,
  getAggregateTokensPerSecond,
  getEstimatedTokensPerSecond,
  getOutputTokensPerSecond,
  getLocaleFromLanguage,
  sumSpeedTotals,
  USD_DISPLAY,
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

  it("resolves Turkish to a Turkish date locale", () => {
    expect(getLocaleFromLanguage("tr")).toBe("tr-TR");
    expect(getLocaleFromLanguage("tr-TR")).toBe("tr-TR");
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
    // 推出了首字的会话日志：仍按整段耗时估，不当成精确速度
    const withFirstToken = { ...session, firstTokenMs: 4_000 };
    expect(getEstimatedTokensPerSecond(withFirstToken)).toBe(90);
    expect(getOutputTokensPerSecond(withFirstToken)).toBeNull();
    expect(sumSpeedTotals([withFirstToken])).toEqual({
      outputTokens: 0,
      generationMs: 0,
    });
  });

  it("formats first-token time in seconds", () => {
    expect(formatFirstTokenSeconds(2_640)).toBe("2.6s");
    expect(formatFirstTokenSeconds(0)).toBe("0.0s");
    expect(formatFirstTokenSeconds(null)).toBeNull();
    expect(formatFirstTokenSeconds(undefined)).toBeNull();
    expect(formatFirstTokenSeconds(-1)).toBeNull();
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

describe("usage money formatting", () => {
  it("keeps the USD output the old fmtUsd produced", () => {
    expect(formatMoney(1.23456, 2, USD_DISPLAY)).toBe("$1.23");
    expect(formatMoney("0.000123", 6, USD_DISPLAY)).toBe("$0.000123");
    expect(formatMoney(null, 4, USD_DISPLAY)).toBe("--");
    expect(formatMoney("x", 4, USD_DISPLAY, "")).toBe("");
  });

  it("trims compact amounts by magnitude", () => {
    expect(
      [0.0081, 0.0123, 1.25].map((usd) => formatMoneyCompact(usd, USD_DISPLAY)),
    ).toEqual(["$0.0081", "$0.012", "$1.25"]);
  });

  it("formats axis ticks without fixing decimals", () => {
    expect(formatMoneyTick(0.5, USD_DISPLAY)).toBe("$0.5");
    expect(formatMoneyTick(undefined, USD_DISPLAY)).toBe("$0");
  });
});

describe("display currency", () => {
  it("falls back to USD and the preset rate for unusable settings", () => {
    expect(resolveMoneyDisplay(undefined)).toEqual(USD_DISPLAY);
    expect(resolveMoneyDisplay({ currency: "GBP" })).toEqual(USD_DISPLAY);
    // 汇率只在显示非美元时生效
    expect(
      resolveMoneyDisplay({ currency: "USD", exchangeRates: { CNY: 7 } }),
    ).toEqual(USD_DISPLAY);
    expect(resolveMoneyDisplay({ currency: "CNY" })).toEqual({
      currency: "CNY",
      rate: 6.8,
    });
    for (const bad of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
      expect(
        resolveMoneyDisplay({ currency: "CNY", exchangeRates: { CNY: bad } })
          .rate,
      ).toBe(6.8);
    }
    expect(
      resolveMoneyDisplay({ currency: "CNY", exchangeRates: { CNY: 7.25 } }),
    ).toEqual({ currency: "CNY", rate: 7.25 });
  });

  it("writes converted amounts with the plain currency symbol", () => {
    const cny = { currency: "CNY" as const, rate: 7 };
    expect(formatMoney(1.5, 2, cny)).toBe("¥10.50");
    expect(formatMoney(0, 2, cny)).toBe("¥0.00");
    expect(formatMoney(null, 2, cny)).toBe("--");
    expect(formatMoneyCompact(0.001, cny)).toBe("¥0.007");
    expect(formatMoneyCompact(0.0015, cny)).toBe("¥0.011");
    // 刻度值画图前已换算，只加符号
    expect(formatMoneyTick(50, cny)).toBe("¥50");
  });

  it("writes each currency with its own symbol and groups thousands", () => {
    expect(formatMoney(30470.55, 2, USD_DISPLAY)).toBe("$30,470.55");
    expect(formatMoney(-1234, 0, USD_DISPLAY)).toBe("$-1,234");
    expect(formatMoney(2, 2, { currency: "EUR", rate: 0.87 })).toBe("€1.74");
    expect(formatMoney(2, 2, { currency: "TWD", rate: 32 })).toBe("NT$64.00");
    expect(formatMoney(100, 2, { currency: "TRY", rate: 48 })).toBe(
      "₺4,800.00",
    );
    expect(formatMoneyTick(20000, { currency: "JPY", rate: 158 })).toBe(
      "¥20,000",
    );
  });

  it("keeps converted amounts to two decimals at most", () => {
    const twd = { currency: "TWD" as const, rate: 32 };
    // 悬停用的 6 位、日志用的 4 位，换算后都只留两位
    expect(formatMoney(64.912461, 6, twd)).toBe("NT$2,077.20");
    expect(formatMoney(0.0505, 4, twd)).toBe("NT$1.62");
    expect(formatMoney(1.5, 1, twd)).toBe("NT$48.0");
    // 小到四舍五入成 0 的一笔写两位有效数字，不写成 0.00
    expect(formatMoney(0.0001, 4, twd)).toBe("NT$0.0032");
    expect(formatMoney(0, 4, twd)).toBe("NT$0.00");
    // 美元照调用方给的位数
    expect(formatMoney(0.0001, 4, USD_DISPLAY)).toBe("$0.0001");
    expect(formatMoney(64.912461, 6, USD_DISPLAY)).toBe("$64.912461");
  });

  it("shows yen with two fewer decimals", () => {
    const jpy = { currency: "JPY" as const, rate: 158 };
    expect(formatMoney(0.0012, 4, jpy)).toBe("¥0.19");
    expect(formatMoney(30470.55, 2, jpy)).toBe("¥4,814,347");
    expect(formatMoney(1.5, 1, jpy)).toBe("¥237");
    expect(formatMoneyShort(0.5, jpy)).toBe("¥79");
  });

  it("drops the decimals in narrow spots once the amount reaches 100", () => {
    const cny = { currency: "CNY" as const, rate: 6.8 };
    expect(formatMoneyShort(41.2, USD_DISPLAY)).toBe("$41.20");
    expect(formatMoneyShort(123.456, USD_DISPLAY)).toBe("$123");
    expect(formatMoneyShort(41.2, cny)).toBe("¥280");
    expect(formatMoneyShort(14.5, cny)).toBe("¥98.60");
    expect(formatMoneyShort(undefined, cny)).toBe("--");
  });
});

describe("sidebar Today items", () => {
  it("shows only the cost until something is picked, and nothing for an empty list", () => {
    expect(resolveSidebarToday(undefined)).toEqual(["cost"]);
    expect(resolveSidebarToday({})).toEqual(["cost"]);
    expect(resolveSidebarToday({ sidebarTodayItems: [] })).toEqual([]);
  });

  it("keeps menu order, drops unknown items and stops at two", () => {
    expect(
      resolveSidebarToday({ sidebarTodayItems: ["hitRate", "cost"] }),
    ).toEqual(["cost", "hitRate"]);
    expect(
      resolveSidebarToday({ sidebarTodayItems: ["successRate", "requests"] }),
    ).toEqual(["requests"]);
    expect(
      resolveSidebarToday({
        sidebarTodayItems: ["hitRate", "requests", "tokens", "cost"],
      }),
    ).toEqual(["cost", "tokens"]);
  });

  it("writes the hit rate like the usage page", () => {
    expect(formatHitRatePercent(0.986)).toBe("98.6%");
    expect(formatHitRatePercent(0.9996)).toBe("100%");
    expect(formatHitRatePercent(0)).toBe("0.0%");
  });
});
