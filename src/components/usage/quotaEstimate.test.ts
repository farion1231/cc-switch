import { describe, it, expect } from "vitest";
import {
  tierWindowMs,
  tierModelPredicate,
  cumulativeQuotaUsd,
  deltaSlopeQuotaUsd,
  combineEstimates,
  foldEma,
  applyDeadband,
  classifyTrend,
  isEstimableApp,
  MIN_OBSERVED_DU,
} from "./quotaEstimate";

const HOUR = 3_600_000;
const DAY = 86_400_000;

describe("tierWindowMs", () => {
  it("maps known window names", () => {
    expect(tierWindowMs("five_hour")).toBe(5 * HOUR);
    expect(tierWindowMs("seven_day")).toBe(7 * DAY);
    expect(tierWindowMs("seven_day_opus")).toBe(7 * DAY);
    expect(tierWindowMs("seven_day_sonnet")).toBe(7 * DAY);
    expect(tierWindowMs("weekly_limit")).toBe(7 * DAY);
    expect(tierWindowMs("30_day")).toBe(30 * DAY);
  });
  it("returns null for unknown windows (skips estimation)", () => {
    expect(tierWindowMs("gemini_pro")).toBeNull();
    expect(tierWindowMs("nonsense")).toBeNull();
  });
});

describe("tierModelPredicate", () => {
  it("opus tier only takes opus models", () => {
    const p = tierModelPredicate("seven_day_opus");
    expect(p("claude-opus-4-8")).toBe(true);
    expect(p("claude-sonnet-5")).toBe(false);
  });
  it("sonnet tier only takes sonnet models", () => {
    const p = tierModelPredicate("seven_day_sonnet");
    expect(p("claude-sonnet-5")).toBe(true);
    expect(p("claude-opus-4-8")).toBe(false);
  });
  it("shared tiers take every model", () => {
    const p = tierModelPredicate("five_hour");
    expect(p("claude-opus-4-8")).toBe(true);
    expect(p("claude-sonnet-5")).toBe(true);
    expect(p("gpt-5")).toBe(true);
  });
});

describe("cumulativeQuotaUsd (lower-bound from all local data)", () => {
  it("reverse-computes 100·C/U", () => {
    expect(cumulativeQuotaUsd(3, 1)).toBeCloseTo(300);
    expect(cumulativeQuotaUsd(12, 30)).toBeCloseTo(40);
  });
  it("returns null on non-positive percentage or spend", () => {
    expect(cumulativeQuotaUsd(5, 0)).toBeNull();
    expect(cumulativeQuotaUsd(5, -1)).toBeNull();
    expect(cumulativeQuotaUsd(0, 15)).toBeNull();
    expect(cumulativeQuotaUsd(NaN, 10)).toBeNull();
  });
});

describe("deltaSlopeQuotaUsd (baseline-robust incremental)", () => {
  it("is null until enough window movement is observed", () => {
    // ΣΔU below the gate → hidden, regardless of ΣΔC
    expect(deltaSlopeQuotaUsd(2, MIN_OBSERVED_DU - 1)).toBeNull();
    expect(deltaSlopeQuotaUsd(0, 10)).toBeNull();
  });
  it("returns 100·ΣΔC/ΣΔU once the gate is met", () => {
    // spent $10 while 5% of the window elapsed → total ≈ $200
    expect(deltaSlopeQuotaUsd(10, 5)).toBeCloseTo(200);
    // the historical baseline is irrelevant: only the accumulated deltas matter
    expect(deltaSlopeQuotaUsd(30, 6)).toBeCloseTo(500);
  });
  it("honors a custom gate", () => {
    expect(deltaSlopeQuotaUsd(10, 8, 10)).toBeNull();
    expect(deltaSlopeQuotaUsd(10, 10, 10)).toBeCloseTo(100);
  });
});

describe("combineEstimates (delta primary, cumulative floor)", () => {
  it("is null while the delta estimate is not yet available", () => {
    expect(combineEstimates(null, 1200)).toBeNull();
    expect(combineEstimates(null, null)).toBeNull();
  });
  it("takes the delta when there is no cumulative floor", () => {
    expect(combineEstimates(1500, null)).toBe(1500);
  });
  it("floors the delta at the cumulative lower bound (uses both)", () => {
    // delta noisy-low → cumulative floor catches it
    expect(combineEstimates(900, 1200)).toBe(1200);
    // multi-surface: delta recovers the true (higher) value, cumulative is loose
    expect(combineEstimates(2000, 800)).toBe(2000);
  });
});

describe("foldEma", () => {
  it("uses the first sample as the initial value", () => {
    expect(foldEma(null, 100)).toBe(100);
  });
  it("converges toward the true value without overshooting", () => {
    let ema: number | null = 100;
    for (let i = 0; i < 20; i++) ema = foldEma(ema, 200);
    expect(ema).toBeGreaterThan(190);
    expect(ema).toBeLessThanOrEqual(200);
  });
  it("smooths noise (does not fully track a single sample)", () => {
    const next = foldEma(100, 300);
    expect(next).toBeGreaterThan(100);
    expect(next).toBeLessThan(300);
  });
});

describe("applyDeadband", () => {
  it("uses the EMA directly when there is no prior display value", () => {
    expect(applyDeadband(null, 1800)).toBe(1800);
    expect(applyDeadband(0, 1800)).toBe(1800);
  });
  it("holds the previous value while the EMA stays within the 2% band", () => {
    expect(applyDeadband(1800, 1819)).toBe(1800);
    expect(applyDeadband(1800, 1780)).toBe(1800);
  });
  it("snaps to the EMA once it drifts past the band", () => {
    expect(applyDeadband(1800, 1850)).toBe(1850);
    expect(applyDeadband(1800, 1750)).toBe(1750);
  });
});

describe("classifyTrend", () => {
  it("treats a missing baseline as flat", () => {
    expect(classifyTrend(null, 100)).toBe("flat");
    expect(classifyTrend(0, 100)).toBe("flat");
  });
  it("treats small moves within the threshold as flat", () => {
    expect(classifyTrend(100, 105)).toBe("flat"); // +5% < 8%
  });
  it("detects significant rises/drops", () => {
    expect(classifyTrend(100, 130)).toBe("up"); // provider raised quota
    expect(classifyTrend(100, 70)).toBe("down"); // provider cut quota
  });
});

describe("isEstimableApp", () => {
  it("only claude / codex are estimable", () => {
    expect(isEstimableApp("claude")).toBe(true);
    expect(isEstimableApp("codex")).toBe(true);
    expect(isEstimableApp("gemini")).toBe(false);
    expect(isEstimableApp(undefined)).toBe(false);
  });
});
