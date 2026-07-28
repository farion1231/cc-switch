import type { UsageActivityDay } from "@/types/usage";

const WEEKDAY_COUNT = 7;

export const ACTIVITY_WEIGHT_TOKENS = 0.7;
export const ACTIVITY_WEIGHT_SESSIONS = 0.3;

export type ActivityIntensity = 0 | 1 | 2 | 3 | 4 | 5;

export interface ActivityHeatmapCell extends UsageActivityDay {
  dateObject: Date;
  dayIndex: number;
  intensity: ActivityIntensity;
}

export interface ActivityHeatmapWeek {
  index: number;
  cells: Array<ActivityHeatmapCell | null>;
}

export interface ActivityHeatmapMatrix {
  weeks: ActivityHeatmapWeek[];
  maxTokens: number;
  maxSessions: number;
  activeDays: number;
  totalTokens: number;
  totalSessions: number;
}

export function parseLocalDateString(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;

  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const parsed = new Date(year, month - 1, day);

  if (
    parsed.getFullYear() !== year ||
    parsed.getMonth() !== month - 1 ||
    parsed.getDate() !== day
  ) {
    return null;
  }

  return parsed;
}

export function getMondayFirstDayIndex(date: Date): number {
  return (date.getDay() + 6) % WEEKDAY_COUNT;
}

export function getActivityScore(
  day: Pick<UsageActivityDay, "realTotalTokens" | "sessionCount">,
  maxTokens: number,
  maxSessions: number,
): number {
  const tokenScore =
    maxTokens > 0
      ? Math.log1p(Math.max(0, day.realTotalTokens)) / Math.log1p(maxTokens)
      : 0;
  const sessionScore =
    maxSessions > 0
      ? Math.log1p(Math.max(0, day.sessionCount)) / Math.log1p(maxSessions)
      : 0;

  return (
    ACTIVITY_WEIGHT_TOKENS * tokenScore +
    ACTIVITY_WEIGHT_SESSIONS * sessionScore
  );
}

export function getActivityIntensity(
  day: Pick<UsageActivityDay, "realTotalTokens" | "sessionCount">,
  maxTokens: number,
  maxSessions: number,
): ActivityIntensity {
  if (day.realTotalTokens <= 0 && day.sessionCount <= 0) return 0;
  const score = getActivityScore(day, maxTokens, maxSessions);
  return Math.max(1, Math.min(5, Math.ceil(score * 5))) as ActivityIntensity;
}

export function buildActivityHeatmap(
  days: UsageActivityDay[],
): ActivityHeatmapMatrix {
  const datedDays = days
    .map((day) => ({ day, dateObject: parseLocalDateString(day.date) }))
    .filter(
      (item): item is { day: UsageActivityDay; dateObject: Date } =>
        item.dateObject != null,
    )
    .sort((a, b) => a.dateObject.getTime() - b.dateObject.getTime());

  const maxTokens = datedDays.reduce(
    (max, item) => Math.max(max, item.day.realTotalTokens),
    0,
  );
  const maxSessions = datedDays.reduce(
    (max, item) => Math.max(max, item.day.sessionCount),
    0,
  );
  const activeDays = datedDays.filter(
    (item) => item.day.realTotalTokens > 0 || item.day.sessionCount > 0,
  ).length;
  const totalTokens = datedDays.reduce(
    (sum, item) => sum + item.day.realTotalTokens,
    0,
  );
  const totalSessions = datedDays.reduce(
    (sum, item) => sum + item.day.sessionCount,
    0,
  );

  if (datedDays.length === 0) {
    return {
      weeks: [],
      maxTokens,
      maxSessions,
      activeDays,
      totalTokens,
      totalSessions,
    };
  }

  const leadingBlanks = getMondayFirstDayIndex(datedDays[0].dateObject);
  const weekCount = Math.ceil(
    (leadingBlanks + datedDays.length) / WEEKDAY_COUNT,
  );
  const weeks: ActivityHeatmapWeek[] = Array.from(
    { length: weekCount },
    (_, index) => ({
      index,
      cells: Array.from(
        { length: WEEKDAY_COUNT },
        (): ActivityHeatmapCell | null => null,
      ),
    }),
  );

  datedDays.forEach((item, offset) => {
    const position = leadingBlanks + offset;
    const weekIndex = Math.floor(position / WEEKDAY_COUNT);
    const dayIndex = position % WEEKDAY_COUNT;
    weeks[weekIndex].cells[dayIndex] = {
      ...item.day,
      dateObject: item.dateObject,
      dayIndex,
      intensity: getActivityIntensity(item.day, maxTokens, maxSessions),
    };
  });

  return {
    weeks,
    maxTokens,
    maxSessions,
    activeDays,
    totalTokens,
    totalSessions,
  };
}
