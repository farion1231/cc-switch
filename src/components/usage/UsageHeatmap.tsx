import { Fragment, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { useUsageTrends } from "@/lib/query/usage";
import { cn } from "@/lib/utils";
import type { UsageRangeSelection } from "@/types/usage";
import { getLocaleFromLanguage, parseFiniteNumber } from "./format";
import type { UsageTrendStatLike } from "./UsageTrendChart";
import { UsageTooltipCard } from "./UsageTooltipCard";

const WEEKS = 53;
const DAY_MS = 24 * 60 * 60 * 1000;
/** 0 档用底色，1–4 档是 --heat-1…4（与趋势图同一色相，色阶在 index.css 按深浅色分别定） */
const LEVELS = [0, 1, 2, 3, 4];

type HeatMetric = "tokens" | "requests" | "cost";

interface UsageHeatmapProps {
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

interface DayCell {
  key: string;
  date: Date;
  value: number;
  tokens: number;
  requests: number;
  cost: number;
  future: boolean;
}

interface HoverState {
  cell: DayCell;
  /** 相对热力图卡片的格子顶部中点 */
  x: number;
  y: number;
}

function startOfDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function dayKey(date: Date): string {
  return `${date.getFullYear()}-${date.getMonth() + 1}-${date.getDate()}`;
}

/** 周一为一周第一天：0 = 周一 … 6 = 周日 */
function weekdayIndex(date: Date): number {
  return (date.getDay() + 6) % 7;
}

/** 热力图起点：往前 52 周那一周的周一，凑满 53 列 */
function getHeatmapStart(now: Date): Date {
  const today = startOfDay(now);
  const thisMonday = new Date(today.getTime() - weekdayIndex(today) * DAY_MS);
  return new Date(
    thisMonday.getFullYear(),
    thisMonday.getMonth(),
    thisMonday.getDate() - (WEEKS - 1) * 7,
  );
}

function metricValue(stat: UsageTrendStatLike, metric: HeatMetric): number {
  if (metric === "requests") return stat.requestCount ?? 0;
  if (metric === "cost") return parseFiniteNumber(stat.totalCost) ?? 0;
  return (
    stat.totalInputTokens +
    stat.totalOutputTokens +
    stat.totalCacheCreationTokens +
    stat.totalCacheReadTokens
  );
}

/** 按非零天的四分位分档，避免个别高峰日把其余格子都压成最浅 */
function buildThresholds(values: number[]): number[] {
  const sorted = values.filter((v) => v > 0).sort((a, b) => a - b);
  if (sorted.length === 0) return [Infinity, Infinity, Infinity];
  const at = (q: number) =>
    sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * q))];
  return [at(0.25), at(0.5), at(0.75)];
}

function levelOf(value: number, thresholds: number[]): number {
  if (value <= 0) return 0;
  if (value <= thresholds[0]) return 1;
  if (value <= thresholds[1]) return 2;
  if (value <= thresholds[2]) return 3;
  return 4;
}

function cellStyle(level: number) {
  return level === 0
    ? undefined
    : {
        backgroundColor: `var(--heat-${level})`,
      };
}

/** 最近 53 周的按天数据：起点按天取整，同一天内查询键不变。 */
function useYearDailyTrends(
  filters: { appType?: string; providerName?: string; model?: string },
  refreshIntervalMs: number,
) {
  const startKey = dayKey(getHeatmapStart(new Date()));
  const selection = useMemo<UsageRangeSelection>(
    () => ({
      preset: "custom",
      customStartDate: Math.floor(getHeatmapStart(new Date()).getTime() / 1000),
      liveEndTime: true,
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [startKey],
  );
  return useUsageTrends(selection, filters, {
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
  });
}

/** 把后端按天的数据铺成 53 周 × 7 天的格子（周一开头，未来的日子标记出来）。 */
function buildWeeks(
  trends: UsageTrendStatLike[] | undefined,
  metric: HeatMetric,
): DayCell[][] {
  const byDay = new Map<string, UsageTrendStatLike>();
  for (const stat of trends ?? []) {
    byDay.set(dayKey(new Date(stat.date)), stat);
  }
  const today = startOfDay(new Date());
  const start = getHeatmapStart(today);
  const weeks: DayCell[][] = [];
  for (let w = 0; w < WEEKS; w++) {
    const column: DayCell[] = [];
    for (let d = 0; d < 7; d++) {
      const date = new Date(
        start.getFullYear(),
        start.getMonth(),
        start.getDate() + w * 7 + d,
      );
      const stat = byDay.get(dayKey(date));
      column.push({
        key: dayKey(date),
        date,
        value: stat ? metricValue(stat, metric) : 0,
        tokens: stat ? metricValue(stat, "tokens") : 0,
        requests: stat?.requestCount ?? 0,
        cost: stat ? metricValue(stat, "cost") : 0,
        future: date.getTime() > today.getTime(),
      });
    }
    weeks.push(column);
  }
  return weeks;
}

export function UsageHeatmap({
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: UsageHeatmapProps) {
  const { t, i18n } = useTranslation();
  const [metric, setMetric] = useState<HeatMetric>("tokens");
  const sectionRef = useRef<HTMLElement>(null);
  const [hover, setHover] = useState<HoverState | null>(null);

  const showTooltip = (cell: DayCell, target: HTMLElement) => {
    const section = sectionRef.current;
    if (!section) return;
    const box = section.getBoundingClientRect();
    const rect = target.getBoundingClientRect();
    setHover({
      cell,
      x: rect.left + rect.width / 2 - box.left,
      y: rect.top - box.top,
    });
  };
  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);

  const { data: trends, isLoading } = useYearDailyTrends(
    { appType, providerName, model },
    refreshIntervalMs,
  );

  const heat = useMemo(() => {
    const weeks = buildWeeks(trends, metric);
    const days = weeks.flat().filter((cell) => !cell.future);
    const thresholds = buildThresholds(days.map((cell) => cell.value));

    // 月份标签：某一列包含当月 1 号（或第一列）时标出月份
    const monthLabels = weeks.map((column, index) => {
      const first = column.find((cell) => cell.date.getDate() === 1);
      if (index === 0) return column[0].date.getMonth() + 1;
      return first ? first.date.getMonth() + 1 : null;
    });

    return { weeks, thresholds, monthLabels };
  }, [trends, metric]);

  const weekdayLabels = useMemo(() => {
    // 2024-01-01 是周一
    return Array.from({ length: 7 }, (_, d) =>
      new Date(2024, 0, 1 + d).toLocaleDateString(locale, {
        weekday: "short",
      }),
    );
  }, [locale]);

  return (
    <section
      ref={sectionRef}
      aria-labelledby="usage-heatmap-title"
      className="relative shrink-0 rounded-panel border border-border bg-surface px-4 py-3"
    >
      <div className="flex flex-wrap items-start gap-x-3.5 gap-y-1">
        <div>
          <h2
            id="usage-heatmap-title"
            className="m-0 text-body font-semibold text-fg-1"
          >
            {t("usage.heatmap.title")}
          </h2>
          <p className="m-0 text-caption text-fg-3">
            {t("usage.heatmap.subtitle")}
          </p>
        </div>
        <div className="flex-1" />
        <SegmentedControl<HeatMetric>
          size="sm"
          aria-label={t("usage.trend.metricLabel")}
          value={metric}
          onValueChange={setMetric}
          items={[
            { value: "tokens", label: t("usage.trend.tokens") },
            { value: "requests", label: t("usage.trend.requests") },
            { value: "cost", label: t("usage.trend.cost") },
          ]}
        />
      </div>

      {isLoading ? (
        <div className="flex h-[150px] items-center justify-center">
          <Loader2 className="h-5 w-5 animate-spin text-fg-3" />
        </div>
      ) : (
        <div
          className="mt-3 grid items-center gap-[3px]"
          style={{
            gridTemplateColumns: `max-content repeat(${WEEKS}, minmax(0, 1fr))`,
          }}
        >
          <span />
          {heat.weeks.map((_, w) => (
            <span
              key={`m${w}`}
              className="h-4 overflow-visible whitespace-nowrap text-badge leading-4 text-fg-3"
            >
              {heat.monthLabels[w] != null
                ? new Date(
                    2024,
                    heat.monthLabels[w]! - 1,
                    1,
                  ).toLocaleDateString(locale, { month: "short" })
                : ""}
            </span>
          ))}
          {weekdayLabels.map((label, d) => (
            <Fragment key={`r${d}`}>
              <span className="pe-1.5 text-badge leading-none text-fg-3">
                {d % 2 === 0 ? label : ""}
              </span>
              {heat.weeks.map((column) => {
                const cell = column[d];
                const level = levelOf(cell.value, heat.thresholds);
                return (
                  <div
                    key={cell.key}
                    onMouseEnter={
                      cell.future
                        ? undefined
                        : (event) => showTooltip(cell, event.currentTarget)
                    }
                    onMouseLeave={() => setHover(null)}
                    className={cn(
                      "aspect-square w-full rounded-[3px]",
                      !cell.future && level === 0 && "bg-subtle",
                    )}
                    style={cell.future ? undefined : cellStyle(level)}
                  />
                );
              })}
            </Fragment>
          ))}
        </div>
      )}

      {hover && (
        <div
          role="tooltip"
          className="pointer-events-none absolute z-20"
          style={{
            left: hover.x,
            top: hover.y - 8,
            // 靠近左右边缘时往里收，别被卡片裁掉
            transform: `translate(${
              hover.x < 110
                ? "-15%"
                : hover.x > (sectionRef.current?.clientWidth ?? 0) - 110
                  ? "-85%"
                  : "-50%"
            }, -100%)`,
          }}
        >
          <UsageTooltipCard
            heading={hover.cell.date.toLocaleDateString(locale, {
              year: "numeric",
              month: "long",
              day: "numeric",
              weekday: "short",
            })}
            tokens={hover.cell.tokens}
            requests={hover.cell.requests}
            cost={hover.cell.cost}
          />
        </div>
      )}

      <div className="mt-2 flex items-center justify-end gap-1.5 text-badge text-fg-3">
        {t("usage.heatmap.less")}
        {LEVELS.map((level) => (
          <span
            key={level}
            className={cn(
              "h-2.5 w-2.5 rounded-[2px]",
              level === 0 && "bg-subtle",
            )}
            style={cellStyle(level)}
          />
        ))}
        {t("usage.heatmap.more")}
      </div>
    </section>
  );
}
