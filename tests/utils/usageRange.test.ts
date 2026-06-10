import { describe, expect, it } from "vitest";
import {
  resolveUsageRange,
  normalizePickerStart,
  normalizePickerEnd,
} from "@/lib/usageRange";

// normalizePickerEnd 内部依赖 Date.now(); 测试时通过注入 now 不可行,
// 但因为它只判断 end 是不是"当天", 我们用伪"now"构造不同日期来间接覆盖两种分支。
// 这里把 normalizePickerEnd 直接当作受测 API 来用, 不再重复实现一份.

describe("normalizePickerStart", () => {
  it("把任意 ts 归一到当地日期 00:00:00", () => {
    const d = new Date("2026-06-10T11:35:42");
    const ts = Math.floor(d.getTime() / 1000);
    const normalized = normalizePickerStart(ts);
    const result = new Date(normalized * 1000);
    expect(result.getFullYear()).toBe(2026);
    expect(result.getMonth()).toBe(5); // June (0-indexed)
    expect(result.getDate()).toBe(10);
    expect(result.getHours()).toBe(0);
    expect(result.getMinutes()).toBe(0);
    expect(result.getSeconds()).toBe(0);
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
  it("end 是当天 → 总是返回当前时刻 (Math.floor(Date.now()/1000))", () => {
    // 构造一个'今天'的 ts
    const today = new Date();
    today.setHours(11, 35, 42, 0);
    const todayTs = Math.floor(today.getTime() / 1000);
    const before = Math.floor(Date.now() / 1000);
    const normalized = normalizePickerEnd(todayTs);
    const after = Math.floor(Date.now() / 1000);
    // 返回值应该是 'now 时刻', 在 before..after 之间
    expect(normalized).toBeGreaterThanOrEqual(before);
    expect(normalized).toBeLessThanOrEqual(after);
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
    // 用户在 time 框输入 18:00, 真实 input box 后端用 normalizeField 包装,
    // 但 normalizePickerEnd 单独调用也应正确处理 18:00 输入 (非 23:59 也非 0:00).
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

  it("end 输入当天 18:00 → 归一到 now 时刻 (不保持 18:00)", () => {
    const today = new Date();
    today.setHours(18, 0, 0, 0);
    const ts = Math.floor(today.getTime() / 1000);
    const before = Math.floor(Date.now() / 1000);
    const normalized = normalizePickerEnd(ts);
    const after = Math.floor(Date.now() / 1000);
    expect(normalized).toBeGreaterThanOrEqual(before);
    expect(normalized).toBeLessThanOrEqual(after);
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
    const resolved = resolveUsageRange({ preset: "today" });
    const todayMidnight = (() => {
      const d = new Date();
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
    const resolved = resolveUsageRange({ preset: "7d" });
    const now = Math.floor(Date.now() / 1000);
    expect(now - resolved.startDate).toBeGreaterThanOrEqual(86400 * 6);
    expect(now - resolved.startDate).toBeLessThanOrEqual(86400 * 7);
    expect(resolved.endDate).toBe(now);
  });
});