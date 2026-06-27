import { describe, expect, it } from "vitest";
import {
  resolveUsageRange,
  normalizePickerStart,
  normalizePickerEnd,
} from "@/lib/usageRange";

// normalizePickerEnd 内部默认取 Date.now(), 但同时接受显式 nowMs 注入;
// 测试用显式 nowMs 取代 wall-clock 容差断言, 消除跨时区/跨秒的 flake。

describe("normalizePickerStart", () => {
  it("把任意 ts 归一到当地日期 00:00:00", () => {
    const d = new Date("2026-06-10T11:35:42Z");
    const ts = Math.floor(d.getTime() / 1000);
    const normalized = normalizePickerStart(ts);
    const result = new Date(normalized * 1000);
    expect(result.getHours()).toBe(0);
    expect(result.getMinutes()).toBe(0);
    expect(result.getSeconds()).toBe(0);
    // 日期部分用 toDateString 比较, 避免时区差异导致 getDate() 漂移
    expect(result.toDateString()).toBe(d.toDateString());
  });

  it("9:00 输入 → 归一到 00:00", () => {
    const d = new Date();
    d.setHours(9, 0, 0, 0);
    const ts = Math.floor(d.getTime() / 1000);
    const normalized = normalizePickerStart(ts);
    const result = new Date(normalized * 1000);
    expect(result.getHours()).toBe(0);
    expect(result.getMinutes()).toBe(0);
  });
});

describe("normalizePickerEnd", () => {
  it("end 是当天 → 总是返回 nowMs 对应的秒", () => {
    const fixedNow = new Date("2026-06-28T11:35:42Z").getTime();
    // 构造一个"今天"的 ts: 任意时分都行, 只要跟 fixedNow 同一天
    const today = new Date(fixedNow);
    today.setHours(11, 35, 42, 0);
    const todayTs = Math.floor(today.getTime() / 1000);
    const normalized = normalizePickerEnd(todayTs, fixedNow);
    expect(normalized).toBe(Math.floor(fixedNow / 1000));
  });

  it("end 是过去日期 → 归一到当天 23:59:59.999", () => {
    const past = new Date();
    past.setDate(past.getDate() - 5);
    past.setHours(0, 0, 0, 0);
    const pastTs = Math.floor(past.getTime() / 1000);
    const normalized = normalizePickerEnd(pastTs);
    const result = new Date(normalized * 1000);
    expect(result.getHours()).toBe(23);
    expect(result.getMinutes()).toBe(59);
    expect(result.getSeconds()).toBe(59);
  });

  it("end 输入 18:00 (非 23:59) 也会被归一, 不让它逃逸到后端", () => {
    const past = new Date();
    past.setDate(past.getDate() - 3);
    past.setHours(18, 0, 0, 0);
    const pastTs = Math.floor(past.getTime() / 1000);
    const normalized = normalizePickerEnd(pastTs);
    const result = new Date(normalized * 1000);
    // 非当天 → 归一到 23:59
    expect(result.getHours()).toBe(23);
    expect(result.getMinutes()).toBe(59);
  });

  it("end 输入当天 18:00 → 归一到 nowMs 时刻 (不保持 18:00)", () => {
    const fixedNow = new Date("2026-06-28T18:00:00Z").getTime();
    const today = new Date(fixedNow);
    today.setHours(18, 0, 0, 0);
    const ts = Math.floor(today.getTime() / 1000);
    const normalized = normalizePickerEnd(ts, fixedNow);
    expect(normalized).toBe(Math.floor(fixedNow / 1000));
  });
});

describe("resolveUsageRange: custom fallback & 其他 preset", () => {
  /* ── usageRange.ts 的兜底 ── */

  it("GUARD: custom + 无 customStart/End → fallback 到今天 00:00 ~ 23:59 (整天)", () => {
    const resolved = resolveUsageRange({ preset: "custom" });
    const endDate = new Date(resolved.endDate * 1000);
    expect(endDate.getHours()).toBe(23);
    expect(endDate.getMinutes()).toBe(59);
    // start fallback 现在也归一到 00:00, 不是 endDate-DAY_SECONDS
    const startDate = new Date(resolved.startDate * 1000);
    expect(startDate.getHours()).toBe(0);
    // Math.floor 把毫秒砍了, 所以差 ≈86399s (而非 86399.999s)
    const diffSeconds = resolved.endDate - resolved.startDate;
    expect(diffSeconds).toBe(86399);
  });

  it("GUARD: custom + 自定义 customStart/End → passthrough", () => {
    const todayMidnight = (() => {
      const d = new Date();
      d.setHours(0, 0, 0, 0);
      return Math.floor(d.getTime() / 1000);
    })();
    const resolved = resolveUsageRange({
      preset: "custom",
      customStartDate: todayMidnight,
      customEndDate: todayMidnight + 43200, // 12:00
    });
    expect(resolved.endDate - todayMidnight).toBe(43200);
  });

  /* ── 其他 preset 未受影响 ── */

  it("CONTROL: preset today → start = 今天 00:00, end > start", () => {
    const nowMs = Date.now();
    const resolved = resolveUsageRange({ preset: "today" }, nowMs);
    const todayMidnight = (() => {
      const d = new Date(nowMs);
      d.setHours(0, 0, 0, 0);
      return Math.floor(d.getTime() / 1000);
    })();
    expect(resolved.startDate).toBe(todayMidnight);
    expect(resolved.endDate).toBeGreaterThan(todayMidnight);
  });

  it("CONTROL: preset 1d → 24h 窗口", () => {
    const resolved = resolveUsageRange({ preset: "1d" });
    expect(resolved.endDate - resolved.startDate).toBe(86400);
  });

  it("CONTROL: preset 7d → start = today-6d, end = now", () => {
    const nowMs = Date.now();
    const resolved = resolveUsageRange({ preset: "7d" }, nowMs);
    const now = Math.floor(nowMs / 1000);
    expect(now - resolved.startDate).toBeGreaterThanOrEqual(86400 * 6);
    expect(now - resolved.startDate).toBeLessThanOrEqual(86400 * 7);
    expect(resolved.endDate).toBe(now);
  });
});