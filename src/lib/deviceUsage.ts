import type { DeviceUsageReport, DeviceUsageRow } from "@/lib/api/deviceUsage";
import type {
  DailyStats,
  UsageSummaryByApp,
  UsageModelStat,
  UsageRangeSelection,
} from "@/types/usage";
import { resolveUsageRange } from "@/lib/usageRange";

export type UsageQuality = "all" | "verified" | "unverified";
const tokens = (r: DeviceUsageRow) =>
  r.input + r.output + r.cacheRead + r.cacheWrite;

/** Snapshots have day precision: filter whole UTC dates, never imply hourly precision. */
export function selectDeviceUsageRows(
  report: DeviceUsageReport | undefined,
  range: UsageRangeSelection,
  appType = "all",
  model?: string,
  quality: UsageQuality = "all",
  now = Date.now(),
) {
  const resolved = resolveUsageRange(range, now, "UTC");
  const start =
    range.preset === "all"
      ? undefined
      : new Date(resolved.startDate * 1000).toISOString().slice(0, 10);
  const end =
    range.preset === "all"
      ? undefined
      : new Date(resolved.endDate * 1000).toISOString().slice(0, 10);
  return (report?.rows ?? []).filter(
    (r) =>
      (!start || r.date >= start) &&
      (!end || r.date <= end) &&
      (appType === "all" || r.appType === appType) &&
      (!model || r.model === model) &&
      (quality === "all" || r.verified === (quality === "verified")),
  );
}

/** Adapt the transport data once; presentation stays in the existing usage components. */
export function deviceUsagePresentation(rows: DeviceUsageRow[]) {
  const apps = new Map<string, UsageSummaryByApp>();
  const days = new Map<string, DailyStats>();
  const models = new Map<string, UsageModelStat>();
  for (const row of rows) {
    const app = apps.get(row.appType) ?? {
      appType: row.appType,
      summary: {
        totalRequests: 0,
        totalCost: "0",
        totalInputTokens: 0,
        totalOutputTokens: 0,
        totalCacheCreationTokens: 0,
        totalCacheReadTokens: 0,
        successRate: 0,
        realTotalTokens: 0,
        cacheHitRate: 0,
      },
    };
    app.summary.totalRequests += row.requests;
    app.summary.totalCost = String(
      Number(app.summary.totalCost) + Number(row.cost),
    );
    app.summary.totalInputTokens += row.input;
    app.summary.totalOutputTokens += row.output;
    app.summary.totalCacheCreationTokens += row.cacheWrite;
    app.summary.totalCacheReadTokens += row.cacheRead;
    app.summary.realTotalTokens += tokens(row);
    const cacheable =
      app.summary.totalInputTokens +
      app.summary.totalCacheReadTokens +
      app.summary.totalCacheCreationTokens;
    app.summary.cacheHitRate = cacheable
      ? app.summary.totalCacheReadTokens / cacheable
      : 0;
    apps.set(row.appType, app);
    const day = days.get(row.date) ?? {
      date: row.date,
      requestCount: 0,
      totalCost: "0",
      totalTokens: 0,
      totalInputTokens: 0,
      totalOutputTokens: 0,
      totalCacheCreationTokens: 0,
      totalCacheReadTokens: 0,
    };
    day.requestCount += row.requests;
    day.totalCost = String(Number(day.totalCost) + Number(row.cost));
    day.totalTokens += tokens(row);
    day.totalInputTokens += row.input;
    day.totalOutputTokens += row.output;
    day.totalCacheReadTokens += row.cacheRead;
    day.totalCacheCreationTokens += row.cacheWrite;
    days.set(row.date, day);
    const stat = models.get(row.model) ?? {
      model: row.model,
      requestCount: 0,
      totalTokens: 0,
      totalCost: "0",
      avgCostPerRequest: "0",
      estimated: false,
    };
    stat.requestCount += row.requests;
    stat.totalTokens += tokens(row);
    stat.totalCost = String(Number(stat.totalCost) + Number(row.cost));
    stat.avgCostPerRequest = String(
      stat.requestCount ? Number(stat.totalCost) / stat.requestCount : 0,
    );
    stat.estimated ||= !row.verified;
    models.set(row.model, stat);
  }
  const daily: UsageModelStat[] = rows.map((row) => ({
    model: row.model,
    date: row.date,
    appType: row.appType,
    verified: row.verified,
    estimated: !row.verified,
    requestCount: row.requests,
    totalTokens: tokens(row),
    totalCost: row.cost,
    avgCostPerRequest: String(
      row.requests ? Number(row.cost) / row.requests : 0,
    ),
  }));
  return {
    apps: [...apps.values()],
    trends: [...days.values()].sort((a, b) => a.date.localeCompare(b.date)),
    models: [...models.values()],
    daily,
    estimated: rows.some((r) => !r.verified),
  };
}
