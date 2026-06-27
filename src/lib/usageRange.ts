import type { UsageRangePreset, UsageRangeSelection } from "@/types/usage";

const DAY_SECONDS = 24 * 60 * 60;
const DAY_MS = DAY_SECONDS * 1000;

export interface ResolvedUsageRange {
  startDate: number;
  endDate: number;
}

/** unix 秒 ↔ Date 互转,与 picker 内部 toTs/fromTs 共享同一精度语义。 */
export function tsToDate(ts: number): Date {
  return new Date(ts * 1000);
}

export function dateToTs(date: Date): number {
  return Math.floor(date.getTime() / 1000);
}

/** 把任意时间戳归到本地当天 00:00:00 的 Date 对象。用 setHours(0,0,0,0) 处理 DST 边界。 */
export function getStartOfLocalDayDate(nowMs: number): Date {
  const date = new Date(nowMs);
  date.setHours(0, 0, 0, 0);
  return date;
}

/** 判断两个 Date 是否是同一天(本地时间)。 */
export function isSameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

/** 把任意时间戳归到本地当天 23:59:59.999 的 Date 对象。 */
export function getEndOfLocalDayDate(nowMs: number): Date {
  const d = new Date(nowMs);
  d.setHours(23, 59, 59, 999);
  return d;
}

/**
 * Picker 写路径的统一归一化(start 00:00 / end today→now / end 过去→23:59)。
 * 所有写入 draft* 的入口(打开 reset、日历点选、input 框)都过此函数,
 * 避免任意 ts 逃逸到后端 compute_rollup_date_bounds。
 */
export function normalizePickerStart(startTs: number): number {
  return dateToTs(getStartOfLocalDayDate(startTs * 1000));
}

export function normalizePickerEnd(
  endTs: number,
  nowMs: number = Date.now(),
): number {
  if (isSameDay(tsToDate(endTs), new Date(nowMs))) {
    return Math.floor(nowMs / 1000);
  }
  return dateToTs(getEndOfLocalDayDate(endTs * 1000));
}

function getPresetLookbackStart(
  preset: Exclude<UsageRangePreset, "today" | "1d" | "custom">,
  nowMs: number,
): number {
  const dayCount = preset === "7d" ? 7 : preset === "14d" ? 14 : 30;
  return dateToTs(getStartOfLocalDayDate(nowMs - (dayCount - 1) * DAY_MS));
}

export function resolveUsageRange(
  selection: UsageRangeSelection,
  nowMs: number = Date.now(),
): ResolvedUsageRange {
  const endDate = Math.floor(nowMs / 1000);

  switch (selection.preset) {
    case "today":
      return {
        startDate: dateToTs(getStartOfLocalDayDate(nowMs)),
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
        selection.customStartDate ?? dateToTs(getStartOfLocalDayDate(nowMs));
      const customEndDate = selection.liveEndTime
        ? endDate
        : (selection.customEndDate ?? dateToTs(getEndOfLocalDayDate(nowMs)));
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
