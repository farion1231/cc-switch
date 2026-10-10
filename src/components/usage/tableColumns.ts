import type { ColumnOption, ColumnVisibility } from "@/types/table";

interface UsageColumn<K extends string = string>
  extends Omit<ColumnOption<K>, "label"> {
  labelKey: string;
  minWidth: number;
}

export const REQUEST_LOG_COLUMNS = [
  { id: "time", labelKey: "usage.time", required: true, minWidth: 90 },
  { id: "app", labelKey: "usage.app", minWidth: 54 },
  { id: "provider", labelKey: "usage.provider", minWidth: 100 },
  { id: "model", labelKey: "usage.model", minWidth: 116 },
  { id: "freshInput", labelKey: "usage.freshInput", minWidth: 50 },
  { id: "outputTokens", labelKey: "usage.outputTokens", minWidth: 44 },
  { id: "cacheReadTokens", labelKey: "usage.cacheReadTokens", minWidth: 52 },
  { id: "cost", labelKey: "usage.cost", minWidth: 54 },
  { id: "speed", labelKey: "usage.speed", minWidth: 60 },
  {
    id: "firstToken",
    labelKey: "usage.firstToken",
    defaultVisible: false,
    minWidth: 72,
  },
] as const satisfies readonly UsageColumn[];

const STATS_METRIC_COLUMNS = [
  { id: "requests", labelKey: "usage.requests", minWidth: 80 },
  { id: "tokens", labelKey: "usage.tokens", minWidth: 85 },
  { id: "cost", labelKey: "usage.cost", minWidth: 85 },
  { id: "successRate", labelKey: "usage.successRate", minWidth: 80 },
  { id: "speed", labelKey: "usage.speed", minWidth: 70 },
] as const;

export const PROVIDER_STATS_COLUMNS = [
  { id: "provider", labelKey: "usage.provider", required: true, minWidth: 220 },
  ...STATS_METRIC_COLUMNS,
] as const satisfies readonly UsageColumn[];

export const MODEL_STATS_COLUMNS = [
  { id: "model", labelKey: "usage.model", required: true, minWidth: 220 },
  ...STATS_METRIC_COLUMNS,
] as const satisfies readonly UsageColumn[];

export type RequestLogColumnId = (typeof REQUEST_LOG_COLUMNS)[number]["id"];
export type ProviderStatsColumnId =
  (typeof PROVIDER_STATS_COLUMNS)[number]["id"];
export type ModelStatsColumnId = (typeof MODEL_STATS_COLUMNS)[number]["id"];

/** One visibility decision drives headers, cells, empty states and width. */
export function getColumnLayout<K extends string>(
  columns: readonly UsageColumn<K>[],
  visibility?: ColumnVisibility<K>,
) {
  const visible = columns.filter(
    (column) =>
      column.required ||
      (visibility?.[column.id] ?? column.defaultVisible ?? true),
  );
  const ids = new Set(visible.map((column) => column.id));
  return {
    isVisible: (id: K) => ids.has(id),
    visibleCount: visible.length,
    minWidth: visible.reduce((sum, column) => sum + column.minWidth, 0),
  };
}
