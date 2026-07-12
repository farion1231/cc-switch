import { describe, expect, it } from "vitest";
import { countdownStr } from "@/components/SubscriptionQuotaFooter";

describe("countdownStr", () => {
  it("formats >= 24h as XdYh", () => {
    const ms = 2 * 24 * 60 * 60 * 1000 + 4 * 60 * 60 * 1000; // 2d 4h
    expect(countdownStr(null, ms)).toBe("2d4h");
  });

  it("formats < 24h as XhYm", () => {
    const ms = 2 * 60 * 60 * 1000 + 30 * 60 * 1000; // 2h 30m
    expect(countdownStr(null, ms)).toBe("2h30m");
  });

  it("formats < 1h as XmYs", () => {
    const ms = 23 * 60 * 1000 + 45 * 1000; // 23m 45s
    expect(countdownStr(null, ms)).toBe("23m45s");
  });

  it("formats < 1m as Xs", () => {
    const ms = 45 * 1000; // 45s
    expect(countdownStr(null, ms)).toBe("45s");
  });

  it("prefers remainsTimeMs over endTime when both available", () => {
    // remainsTimeMs=1h 切确；endTime 5h - now 较准。
    const ms = 60 * 60 * 1000;
    const futureIso = new Date(Date.now() + 5 * 60 * 60 * 1000).toISOString();
    expect(countdownStr(futureIso, ms)).toBe("1h0m");
  });

  it("falls back to endTime - now when remainsTimeMs absent", () => {
    // 30m 后: totalSeconds=1800, minutes=30, seconds=0 → "30m0s"
    const futureIso = new Date(Date.now() + 30 * 60 * 1000).toISOString();
    expect(countdownStr(futureIso)).toBe("30m0s");
  });

  it("returns null when remainsTimeMs is non-positive or absent and endTime is null", () => {
    expect(countdownStr(null)).toBeNull();
    expect(countdownStr(null, 0)).toBeNull();
    expect(countdownStr(null, -1)).toBeNull();
  });
});
