import { describe, expect, it } from "vitest";
import {
  elapsedPercent,
  formatUsedElapsedPercent,
  isOverPace,
  splitUsedElapsedPercent,
  tierElapsedPercent,
  tierWindowSeconds,
} from "@/utils/quotaWindow";

describe("quotaWindow", () => {
  it("maps known tier names to window seconds", () => {
    expect(tierWindowSeconds("five_hour")).toBe(5 * 3600);
    expect(tierWindowSeconds("seven_day")).toBe(7 * 24 * 3600);
    expect(tierWindowSeconds("weekly_limit")).toBe(7 * 24 * 3600);
    expect(tierWindowSeconds("30_day")).toBe(30 * 24 * 3600);
    expect(tierWindowSeconds("2_hour")).toBe(2 * 3600);
    expect(tierWindowSeconds("3_day")).toBe(3 * 24 * 3600);
    expect(tierWindowSeconds("gemini_pro")).toBeUndefined();
  });

  it("computes elapsed percent from resetsAt", () => {
    const now = 1_700_000_000_000;
    // 5h window, 1h remaining → 80% elapsed
    const resetsAt = new Date(now + 3600 * 1000).toISOString();
    expect(elapsedPercent(5 * 3600, resetsAt, now)).toBeCloseTo(80, 5);
    expect(tierElapsedPercent("five_hour", resetsAt, now)).toBeCloseTo(80, 5);
  });

  it("omits elapsed when reset is farther out than the assumed window", () => {
    // Grok 把 4–12 天后的重置都标成 weekly_limit；10 天后重置配 7 天窗口
    // 会算出负剩余 → 旧实现 clamp 成 0%，让任何非零用量都被误判超进度。
    const now = 1_700_000_000_000;
    const tenDays = new Date(now + 10 * 24 * 3600 * 1000).toISOString();
    expect(tierElapsedPercent("weekly_limit", tenDays, now)).toBeUndefined();
    // 刚重置（剩余 ≈ 窗口长）落在容差内，仍算 0%
    const justReset = new Date(now + 7 * 24 * 3600 * 1000).toISOString();
    expect(tierElapsedPercent("weekly_limit", justReset, now)).toBeCloseTo(
      0,
      5,
    );
  });

  it("returns undefined without reset or window", () => {
    expect(elapsedPercent(5 * 3600, null, Date.now())).toBeUndefined();
    expect(
      tierElapsedPercent("gemini_pro", new Date().toISOString()),
    ).toBeUndefined();
  });

  it("formats used%-elapsed%", () => {
    expect(formatUsedElapsedPercent(9.4, 40.2)).toBe("9%-40%");
    expect(formatUsedElapsedPercent(9.4, undefined)).toBe("9%");
  });

  it("detects over-pace when usage exceeds elapsed time", () => {
    expect(isOverPace(80, 20)).toBe(true);
    expect(isOverPace(20, 80)).toBe(false);
    expect(isOverPace(50, 50)).toBe(false);
    expect(isOverPace(50, undefined)).toBe(false);
  });

  it("splitUsedElapsedPercent marks overPace for bold UI", () => {
    const over = splitUsedElapsedPercent(80, 20);
    expect(over.usedText).toBe("80%");
    expect(over.elapsedText).toBe("20%");
    expect(over.plain).toBe("80%-20%");
    expect(over.overPace).toBe(true);

    const under = splitUsedElapsedPercent(10, 20);
    expect(under.overPace).toBe(false);
    expect(under.plain).toBe("10%-20%");
  });
});
