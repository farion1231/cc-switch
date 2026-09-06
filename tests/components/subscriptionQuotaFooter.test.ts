import { afterEach, describe, expect, it, vi } from "vitest";
import { countdownStr } from "@/components/SubscriptionQuotaFooter";

describe("countdownStr", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("treats offset-free reset timestamps as UTC", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-12T13:00:00Z"));
    // 无时区的 16:00 应按 UTC 解释 → 还剩 3 小时，而非随本地时区漂移
    expect(countdownStr("2026-08-12T16:00:00")).toBe("3h0m");
    expect(countdownStr("2026-08-12 16:00:00")).toBe("3h0m");
  });

  it("still honors explicit offsets", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-12T13:00:00Z"));
    expect(countdownStr("2026-08-13T00:00:00+08:00")).toBe("3h0m");
    expect(countdownStr("2026-08-12T16:00:00Z")).toBe("3h0m");
  });

  it("returns null for missing or already elapsed resets", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-12T13:00:00Z"));
    expect(countdownStr(null)).toBeNull();
    expect(countdownStr("2026-08-12T12:00:00Z")).toBeNull();
  });
});
