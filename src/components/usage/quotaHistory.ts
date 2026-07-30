/**
 * 额度历史的纯逻辑层（fork 附加功能）。
 *
 * 持久化本身在后端：SQLite `~/.cc-switch/cc-switch.db` 的 `fork_quota_history`
 * 表（见 `src-tauri/src/database/dao/quota_history.rs`，惰性建表、不进迁移链）。
 * 这里只做两件不需要碰 IO 的事：
 *
 * 1. 把一份实时 `SubscriptionQuota` 读数抽成可入库的窗口样本；
 * 2. 把后端返回的扁平行组装成 recharts 要的图表数据。
 *
 * 两者都是纯函数，所以分桶、断档、缺失窗口的处理都能脱离 DOM 与 IPC 单测。
 */
import type { SubscriptionQuota } from "@/types/subscription";
import type { QuotaHistoryRow, QuotaTierSample } from "@/lib/api/quotaHistory";

const HOUR_MS = 3_600_000;

/** 相邻样本超过这个小时数就画成断线 */
export const DEFAULT_GAP_HOURS = 2;

/** 毫秒时间戳 → 纪元小时序号 */
export function hourIndex(ms: number): number {
  return Math.floor(ms / HOUR_MS);
}

/** 纪元小时序号 → 该小时起点的毫秒时间戳 */
export function hourStartMs(hour: number): number {
  return hour * HOUR_MS;
}

function finiteOrNull(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/**
 * 从一份实时读数抽出可入库的窗口样本。
 *
 * 返回 null 表示不值得记录：查询失败、没凭据、或没有任何可用窗口。
 * `measuredAt` 用 `queriedAt`（读数的**测量**时间）而不是当前时间——按测量时间
 * 分桶，重复观察到同一份缓存快照才是幂等的，不会把陈旧读数抹到后续每个小时。
 */
export function toQuotaSamples(
  quota: SubscriptionQuota | undefined,
  nowMs: number,
): { measuredAt: number; tiers: QuotaTierSample[] } | null {
  if (!quota?.success) return null;
  const measuredAt = quota.queriedAt ?? nowMs;
  if (!Number.isFinite(measuredAt)) return null;

  const tiers: QuotaTierSample[] = [];
  for (const tier of quota.tiers ?? []) {
    const utilization = finiteOrNull(tier?.utilization);
    if (!tier?.name || utilization == null || utilization < 0) continue;
    tiers.push({
      name: tier.name,
      utilization,
      usedUsd: finiteOrNull(tier.usedValueUsd),
      maxUsd: finiteOrNull(tier.maxValueUsd),
    });
  }
  if (tiers.length === 0) return null;

  return { measuredAt, tiers };
}

/** 图表的一行：ts + 每个窗口的使用率（null 代表断档） */
export interface QuotaSeriesRow {
  ts: number;
  [key: string]: number | null;
}

export function usdUsedKey(tier: string): string {
  return `${tier}__usd`;
}
export function usdMaxKey(tier: string): string {
  return `${tier}__usdMax`;
}

export interface QuotaSeries {
  /** 该区间内出现过的窗口名，按首次出现顺序 */
  tiers: string[];
  rows: QuotaSeriesRow[];
}

/**
 * 把后端的扁平行组装成图表数据。
 *
 * 应用关着的时段没有样本，这种断档会插入一个全 null 的点，让线真的断开——
 * 否则 recharts 会在相隔几天的两点间画一条直线，看起来像「额度稳定」。
 */
export function rowsToSeries(
  rows: QuotaHistoryRow[],
  appId: string,
  gapHours: number = DEFAULT_GAP_HOURS,
): QuotaSeries {
  const byHour = new Map<number, QuotaHistoryRow[]>();
  for (const row of rows) {
    if (row.appId !== appId) continue;
    const bucket = byHour.get(row.hour);
    if (bucket) bucket.push(row);
    else byHour.set(row.hour, [row]);
  }

  const hours = Array.from(byHour.keys()).sort((a, b) => a - b);
  const tiers: string[] = [];
  const seriesRows: QuotaSeriesRow[] = [];
  let prevHour: number | null = null;

  for (const hour of hours) {
    if (prevHour != null && hour - prevHour > gapHours) {
      seriesRows.push({ ts: hourStartMs(prevHour + 1) });
    }
    const row: QuotaSeriesRow = { ts: hourStartMs(hour) };
    for (const entry of byHour.get(hour) ?? []) {
      if (!tiers.includes(entry.tier)) tiers.push(entry.tier);
      row[entry.tier] = entry.utilization;
      if (entry.usedUsd != null) row[usdUsedKey(entry.tier)] = entry.usedUsd;
      if (entry.maxUsd != null) row[usdMaxKey(entry.tier)] = entry.maxUsd;
    }
    seriesRows.push(row);
    prevHour = hour;
  }

  // 某个窗口只在部分小时出现时，其余行必须是显式 null，否则 recharts 会把上一个
  // 点一路平推过去，看着像那段时间额度没动。
  for (const row of seriesRows) {
    for (const tier of tiers) {
      if (!(tier in row)) row[tier] = null;
    }
  }

  return { tiers, rows: seriesRows };
}

/** 按给定优先级挑出有数据的应用 */
export function appsWithHistory(
  rows: QuotaHistoryRow[],
  candidates: readonly string[],
): string[] {
  const present = new Set(rows.map((row) => row.appId));
  return candidates.filter((appId) => present.has(appId));
}
