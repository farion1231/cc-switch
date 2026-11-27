import { describe, expect, it } from "vitest";
import { monthLabelsOf } from "@/components/usage/UsageHeatmap";

/** 从某个周一起铺 n 列，每列 7 天 */
function weeksFrom(year: number, month: number, day: number, count: number) {
  return Array.from({ length: count }, (_, w) =>
    Array.from({ length: 7 }, (_, d) => ({
      date: new Date(year, month - 1, day + w * 7 + d),
    })),
  );
}

describe("monthLabelsOf", () => {
  it("drops the first-column label when the next month starts right after it", () => {
    // 2025-11-24 是周一，第二列就含 12 月 1 号（#9917：11月和12月叠在一起）
    const labels = monthLabelsOf(weeksFrom(2025, 11, 24, 6));
    expect(labels.slice(0, 2)).toEqual([null, 12]);
  });

  it("drops it too when the first month only covers two columns", () => {
    // 2025-11-17 起：12 月 1 号在第三列
    const labels = monthLabelsOf(weeksFrom(2025, 11, 17, 6));
    expect(labels.slice(0, 3)).toEqual([null, null, 12]);
  });

  it("keeps the first-column label when there is room for it", () => {
    // 2025-11-10 起：12 月 1 号在第四列
    const labels = monthLabelsOf(weeksFrom(2025, 11, 10, 6));
    expect(labels.slice(0, 4)).toEqual([11, null, null, 12]);
  });

  it("labels the first column with the month whose 1st it contains", () => {
    // 2025-09-29 是周一，第一列含 10 月 1 号（周三）
    const labels = monthLabelsOf(weeksFrom(2025, 9, 29, 6));
    expect(labels[0]).toBe(10);
  });
});
