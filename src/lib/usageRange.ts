import type { UsageRangePreset, UsageRangeSelection } from "@/types/usage";

const DAY_SECONDS = 24 * 60 * 60;
const DAY_MS = DAY_SECONDS * 1000;

export interface ResolvedUsageRange {
  startDate: number;
  endDate: number;
}

/**
 * 把任意时间戳归到本地当天 00:00:00 的 Date 对象。
 * 用 setHours(0,0,0,0) 处理 DST 边界 (而不是依赖 getDate() 的隐式 0 时分秒)。
 */
export function getStartOfLocalDayDate(nowMs: number): Date {
  const date = new Date(nowMs);
  date.setHours(0, 0, 0, 0);
  return date;
}

/**
 * 判断两个 Date 是否是同一天（本地时间）。
 */
export function isSameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

/**
 * 把任意时间戳归到本地当天 23:59:59.999 的 Date 对象。
 * 用于 custom 范围的默认 end：对齐后端"end==23:59 视为当天结束"的隐式约定，
 * 避免 0:00 触发 `pred_opt()` 把 end 推到昨天、start > end 被判为空。
 */
export function getEndOfLocalDayDate(nowMs: number): Date {
  const date = new Date(nowMs);
  // 本地日历加 1 天 (自动适配夏令时缩短或拉长的那一天)
  date.setDate(date.getDate() + 1);
  // 将明天的这一天归零至 00:00:00.000
  date.setHours(0, 0, 0, 0);
  // 减去 1ms，即可安全可靠地得到当天的 23:59:59.999
  return new Date(date.getTime() - 1);
}

/**
 * Picker reset / 日历点选 / time 框输入的统一归一化.
 *
 * 语义:
 *   - start 永远归一到 00:00:00
 *   - end:
 *       * end 所在日期 == 今天 → 总是 now 时刻, 不论原值 (0:00 / 18:00 等都归一)
 *       * end 所在日期 != 今天 → 23:59:59 (整天已过完)
 *
 * 用于 picker 内的所有 '写 draft*' 路径, 防止任意 ts 逃逸到后端
 * (后端 compute_rollup_date_bounds 对 hour==23 && minute==59 敏感)。
 */
export function normalizePickerStart(startTs: number): number {
  return Math.floor(getStartOfLocalDayDate(startTs * 1000).getTime() / 1000);
}

export function normalizePickerEnd(
  endTs: number,
  nowMs: number = Date.now(),
): number {
  const endDate = new Date(endTs * 1000);
  const today = new Date(nowMs);
  if (isSameDay(endDate, today)) {
    // end 是当天 → 总是 now 时刻, 不论原值
    return Math.floor(nowMs / 1000);
  }
  return Math.floor(getEndOfLocalDayDate(endTs * 1000).getTime() / 1000);
}

function getPresetLookbackStart(
  preset: Exclude<UsageRangePreset, "today" | "1d" | "custom">,
  nowMs: number,
): number {
  const dayCount = preset === "7d" ? 7 : preset === "14d" ? 14 : 30;
  return Math.floor(
    getStartOfLocalDayDate(nowMs - (dayCount - 1) * DAY_MS).getTime() / 1000,
  );
}

export function resolveUsageRange(
  selection: UsageRangeSelection,
  nowMs: number = Date.now(),
): ResolvedUsageRange {
  const endDate = Math.floor(nowMs / 1000);

  switch (selection.preset) {
    case "today":
      return {
        startDate: Math.floor(getStartOfLocalDayDate(nowMs).getTime() / 1000),
        endDate,
      };
    case "1d":
      return {
        startDate: endDate - DAY_SECONDS,
        endDate,
      };
    case "7d":
    case "14d":
    case "30d":
      return {
        startDate: getPresetLookbackStart(selection.preset, nowMs),
        endDate,
      };
    case "custom": {
      const startDate =
        selection.customStartDate ??
        Math.floor(getStartOfLocalDayDate(nowMs).getTime() / 1000);
      const customEndDate =
        selection.customEndDate ??
        Math.floor(getEndOfLocalDayDate(nowMs).getTime() / 1000);
      return {
        startDate,
        endDate: customEndDate,
      };
    }
  }
}

export function getUsageRangePresetLabel(
  preset: UsageRangePreset,
  t: (key: string, options?: { defaultValue?: string }) => string,
): string {
  switch (preset) {
    case "today":
      return t("usage.presetToday", { defaultValue: "当天" });
    case "1d":
      return t("usage.preset1d", { defaultValue: "1d" });
    case "7d":
      return t("usage.preset7d", { defaultValue: "7d" });
    case "14d":
      return t("usage.preset14d", { defaultValue: "14d" });
    case "30d":
      return t("usage.preset30d", { defaultValue: "30d" });
    case "custom":
      return t("usage.customRange", { defaultValue: "日历筛选" });
  }
}
