import type { UsageRangePreset, UsageRangeSelection } from "@/types/usage";

const DAY_SECONDS = 24 * 60 * 60;
const DAY_MS = DAY_SECONDS * 1000;

export interface ResolvedUsageRange {
  startDate: number;
  endDate: number;
}

function getStartOfLocalDayDate(nowMs: number): Date {
  const date = new Date(nowMs);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function getPresetLookbackStart(
  preset: Exclude<UsageRangePreset, "today" | "1d" | "all" | "custom">,
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
  timeZone: "local" | "UTC" = "local",
): ResolvedUsageRange {
  const endDate = Math.floor(nowMs / 1000);

  if (
    timeZone === "UTC" &&
    selection.preset !== "all" &&
    selection.preset !== "custom"
  ) {
    const now = new Date(nowMs);
    const today =
      Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate()) /
      1000;
    const days =
      selection.preset === "7d"
        ? 7
        : selection.preset === "14d"
          ? 14
          : selection.preset === "30d"
            ? 30
            : 1;
    return { startDate: today - (days - 1) * DAY_SECONDS, endDate };
  }
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
    // 全部：从最早的记录算起，汇总和明细表按全量统计
    case "all":
      return { startDate: 0, endDate };
    case "custom": {
      const startDate = selection.customStartDate ?? endDate - DAY_SECONDS;
      const customEndDate = selection.liveEndTime
        ? endDate
        : (selection.customEndDate ?? endDate);
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
    case "all":
      return t("usage.presetAll", { defaultValue: "全部" });
    case "custom":
      return t("usage.customRange", { defaultValue: "日历筛选" });
  }
}
