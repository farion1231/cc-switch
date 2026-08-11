import { describe, expect, it } from "vitest";
import {
  elapsedPercent,
  formatUsedElapsedPercent,
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

  it("returns undefined without reset or window", () => {
    expect(elapsedPercent(5 * 3600, null, Date.now())).toBeUndefined();
    expect(tierElapsedPercent("gemini_pro", new Date().toISOString())).toBeUndefined();
  });

  it("formats used%-elapsed%", () => {
    expect(formatUsedElapsedPercent(9.4, 40.2)).toBe("9%-40%");
    expect(formatUsedElapsedPercent(9.4, undefined)).toBe("9%");
  });
});
