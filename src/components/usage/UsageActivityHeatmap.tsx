import type {
  CSSProperties,
  FocusEvent as ReactFocusEvent,
  MouseEvent as ReactMouseEvent,
} from "react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { createPortal } from "react-dom";
import { Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useUsageActivityHeatmap } from "@/lib/query/usage";
import type { AppType } from "@/types/usage";
import {
  fmtInt,
  fmtUsd,
  formatTokensShort,
  getLocaleFromLanguage,
  getResolvedLang,
} from "./format";
import {
  buildActivityHeatmap,
  type ActivityHeatmapCell,
  type ActivityHeatmapWeek,
  type ActivityIntensity,
} from "./activityHeatmap";

interface UsageActivityHeatmapProps {
  appType?: string;
  refreshIntervalMs: number;
}

interface ActivityTooltipState {
  cell: ActivityHeatmapCell;
  left: number;
  top: number;
}

const TOOLTIP_WIDTH = 224;
const TOOLTIP_ESTIMATED_HEIGHT = 148;
const TOOLTIP_GAP = 12;
const VIEWPORT_PADDING = 8;

const INTENSITY_CLASSES: Record<
  AppType | "all",
  Record<ActivityIntensity, string>
> = {
  all: {
    0: "border-border/50 bg-muted/50",
    1: "border-emerald-500/15 bg-emerald-500/20",
    2: "border-emerald-500/20 bg-emerald-500/35",
    3: "border-emerald-500/25 bg-emerald-500/55",
    4: "border-emerald-500/30 bg-emerald-500/75",
    5: "border-emerald-500/40 bg-emerald-600",
  },
  claude: {
    0: "border-border/50 bg-muted/50",
    1: "border-amber-500/15 bg-amber-500/20",
    2: "border-amber-500/20 bg-amber-500/35",
    3: "border-amber-500/25 bg-amber-500/55",
    4: "border-amber-500/30 bg-amber-500/75",
    5: "border-amber-500/40 bg-amber-600",
  },
  codex: {
    0: "border-border/50 bg-muted/50",
    1: "border-emerald-500/15 bg-emerald-500/20",
    2: "border-emerald-500/20 bg-emerald-500/35",
    3: "border-emerald-500/25 bg-emerald-500/55",
    4: "border-emerald-500/30 bg-emerald-500/75",
    5: "border-emerald-500/40 bg-emerald-600",
  },
  gemini: {
    0: "border-border/50 bg-muted/50",
    1: "border-sky-500/15 bg-sky-500/20",
    2: "border-sky-500/20 bg-sky-500/35",
    3: "border-sky-500/25 bg-sky-500/55",
    4: "border-sky-500/30 bg-sky-500/75",
    5: "border-sky-500/40 bg-sky-600",
  },
  grokbuild: {
    0: "border-border/50 bg-muted/50",
    1: "border-rose-500/15 bg-rose-500/20",
    2: "border-rose-500/20 bg-rose-500/35",
    3: "border-rose-500/25 bg-rose-500/55",
    4: "border-rose-500/30 bg-rose-500/75",
    5: "border-rose-500/40 bg-rose-600",
  },
  opencode: {
    0: "border-border/50 bg-muted/50",
    1: "border-violet-500/15 bg-violet-500/20",
    2: "border-violet-500/20 bg-violet-500/35",
    3: "border-violet-500/25 bg-violet-500/55",
    4: "border-violet-500/30 bg-violet-500/75",
    5: "border-violet-500/40 bg-violet-600",
  },
};

function getFilterType(appType?: string): AppType | "all" {
  if (
    appType === "claude" ||
    appType === "codex" ||
    appType === "gemini" ||
    appType === "grokbuild" ||
    appType === "opencode"
  ) {
    return appType;
  }
  return "all";
}

function getMonthLabels(
  weeks: ActivityHeatmapWeek[],
  formatter: Intl.DateTimeFormat,
): string[] {
  let lastMonthKey = "";
  return weeks.map((week) => {
    const candidate = week.cells.find(
      (cell) => cell && cell.dateObject.getDate() <= 7,
    );
    if (!candidate) return "";

    const monthKey = `${candidate.dateObject.getFullYear()}-${candidate.dateObject.getMonth()}`;
    if (monthKey === lastMonthKey) return "";
    lastMonthKey = monthKey;
    return formatter.format(candidate.dateObject);
  });
}

function getWeekdayLabels(formatter: Intl.DateTimeFormat): string[] {
  const monday = new Date(2026, 0, 5);
  return Array.from({ length: 7 }, (_, offset) => {
    const day = new Date(monday);
    day.setDate(monday.getDate() + offset);
    return formatter.format(day);
  });
}

function getTooltipPosition(clientX: number, clientY: number) {
  if (typeof window === "undefined") {
    return { left: clientX + TOOLTIP_GAP, top: clientY + TOOLTIP_GAP };
  }

  const maxLeft = Math.max(
    VIEWPORT_PADDING,
    window.innerWidth - TOOLTIP_WIDTH - VIEWPORT_PADDING,
  );
  const left = Math.min(
    Math.max(VIEWPORT_PADDING, clientX + TOOLTIP_GAP),
    maxLeft,
  );
  const wouldOverflowBottom =
    clientY + TOOLTIP_GAP + TOOLTIP_ESTIMATED_HEIGHT >
    window.innerHeight - VIEWPORT_PADDING;
  const top = wouldOverflowBottom
    ? Math.max(
        VIEWPORT_PADDING,
        clientY - TOOLTIP_GAP - TOOLTIP_ESTIMATED_HEIGHT,
      )
    : clientY + TOOLTIP_GAP;

  return { left, top };
}

function formatCellDate(cell: ActivityHeatmapCell, locale: string): string {
  return cell.dateObject.toLocaleDateString(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
    weekday: "short",
  });
}

function buildCellTitle(
  cell: ActivityHeatmapCell,
  appLabel: string,
  locale: string,
  labels: {
    date: string;
    app: string;
    tokens: string;
    sessions: string;
    requests: string;
    cost: string;
  },
): string {
  return [
    `${labels.date}: ${formatCellDate(cell, locale)}`,
    `${labels.app}: ${appLabel}`,
    `${labels.tokens}: ${fmtInt(cell.realTotalTokens, locale)}`,
    `${labels.sessions}: ${fmtInt(cell.sessionCount, locale)}`,
    `${labels.requests}: ${fmtInt(cell.requestCount, locale)}`,
    `${labels.cost}: ${fmtUsd(cell.totalCost, 6)}`,
  ].join("\n");
}

export function UsageActivityHeatmap({
  appType,
  refreshIntervalMs,
}: UsageActivityHeatmapProps) {
  const { t, i18n } = useTranslation();
  const lang = getResolvedLang(i18n);
  const locale = getLocaleFromLanguage(lang);
  const filterType = getFilterType(appType);
  const appLabel = t(`usage.appFilter.${filterType}`);
  const [tooltip, setTooltip] = useState<ActivityTooltipState | null>(null);

  const { data, isLoading, isError } = useUsageActivityHeatmap(appType, {
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
  });

  const matrix = useMemo(() => buildActivityHeatmap(data ?? []), [data]);
  const monthFormatter = useMemo(
    () => new Intl.DateTimeFormat(locale, { month: "short" }),
    [locale],
  );
  const weekdayFormatter = useMemo(
    () => new Intl.DateTimeFormat(locale, { weekday: "short" }),
    [locale],
  );
  const monthLabels = useMemo(
    () => getMonthLabels(matrix.weeks, monthFormatter),
    [matrix.weeks, monthFormatter],
  );
  const weekdayLabels = useMemo(
    () => getWeekdayLabels(weekdayFormatter),
    [weekdayFormatter],
  );
  const gridStyle: CSSProperties = {
    gridTemplateColumns: `repeat(${Math.max(matrix.weeks.length, 1)}, 0.75rem)`,
    gridTemplateRows: "repeat(7, 0.75rem)",
  };
  const intensityClasses = INTENSITY_CLASSES[filterType];
  const tooltipLabels = useMemo(
    () => ({
      date: t("usage.activityHeatmap.tooltipDate"),
      app: t("usage.activityHeatmap.tooltipApp"),
      tokens: t("usage.activityHeatmap.tooltipTokens"),
      sessions: t("usage.activityHeatmap.tooltipSessions"),
      requests: t("usage.activityHeatmap.tooltipRequests"),
      cost: t("usage.activityHeatmap.tooltipCost"),
    }),
    [t],
  );

  const showTooltip = (
    cell: ActivityHeatmapCell,
    clientX: number,
    clientY: number,
  ) => {
    setTooltip({ cell, ...getTooltipPosition(clientX, clientY) });
  };

  const handleCellMouseEnter =
    (cell: ActivityHeatmapCell) =>
    (event: ReactMouseEvent<HTMLButtonElement>) => {
      showTooltip(cell, event.clientX, event.clientY);
    };

  const handleCellMouseMove =
    (cell: ActivityHeatmapCell) =>
    (event: ReactMouseEvent<HTMLButtonElement>) => {
      showTooltip(cell, event.clientX, event.clientY);
    };

  const handleCellFocus =
    (cell: ActivityHeatmapCell) =>
    (event: ReactFocusEvent<HTMLButtonElement>) => {
      const rect = event.currentTarget.getBoundingClientRect();
      showTooltip(cell, rect.left + rect.width / 2, rect.bottom);
    };

  if (isLoading) {
    return (
      <div className="mt-6 border-t border-border/50 pt-5">
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
          {t("usage.activityHeatmap.loading")}
        </div>
        <div className="mt-3 flex gap-1 overflow-hidden">
          {Array.from({ length: 42 }, (_, index) => (
            <div
              key={index}
              className="h-3 w-3 shrink-0 rounded-[3px] border border-border/40 bg-muted/50"
            />
          ))}
        </div>
      </div>
    );
  }

  if (isError) {
    return (
      <div className="mt-6 border-t border-border/50 pt-5 text-xs text-muted-foreground">
        {t("usage.activityHeatmap.error")}
      </div>
    );
  }

  return (
    <div className="mt-6 border-t border-border/50 pt-5">
      <div className="mb-3 flex flex-wrap items-end justify-between gap-3">
        <div>
          <div className="text-sm font-semibold">
            {t("usage.activityHeatmap.title")}
          </div>
          <div className="mt-0.5 text-xs text-muted-foreground">
            {t("usage.activityHeatmap.summary", {
              activeDays: fmtInt(matrix.activeDays, locale),
              sessions: fmtInt(matrix.totalSessions, locale),
              tokens: formatTokensShort(matrix.totalTokens, lang),
            })}
          </div>
        </div>
        <div className="flex items-center gap-1 text-[11px] text-muted-foreground">
          <span>{t("usage.activityHeatmap.less")}</span>
          {[0, 1, 2, 3, 4, 5].map((level) => (
            <span
              key={level}
              className={cn(
                "h-3 w-3 rounded-[3px] border",
                intensityClasses[level as ActivityIntensity],
              )}
            />
          ))}
          <span>{t("usage.activityHeatmap.more")}</span>
        </div>
      </div>

      <div className="overflow-x-auto pb-1">
        <div className="min-w-max">
          <div
            className="mb-1 ml-8 hidden gap-1 md:grid"
            style={{
              gridTemplateColumns: `repeat(${Math.max(matrix.weeks.length, 1)}, 0.75rem)`,
            }}
          >
            {monthLabels.map((label, index) => (
              <span
                key={`${label}-${index}`}
                className="h-3 whitespace-nowrap text-[10px] leading-3 text-muted-foreground"
              >
                {label}
              </span>
            ))}
          </div>

          <div className="flex gap-2">
            <div className="hidden grid-rows-7 gap-1 md:grid">
              {weekdayLabels.map((label, index) => (
                <span
                  key={label}
                  className="h-3 w-6 text-[10px] leading-3 text-muted-foreground"
                >
                  {index === 0 || index === 2 || index === 4 ? label : ""}
                </span>
              ))}
            </div>

            <div
              className="grid grid-flow-col gap-1"
              style={gridStyle}
              aria-label={t("usage.activityHeatmap.title")}
            >
              {matrix.weeks.flatMap((week) =>
                week.cells.map((cell, dayIndex) =>
                  cell ? (
                    <button
                      key={cell.date}
                      type="button"
                      className={cn(
                        "h-3 w-3 cursor-default appearance-none rounded-[3px] border p-0 transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-1 focus:ring-offset-background",
                        intensityClasses[cell.intensity],
                      )}
                      aria-label={buildCellTitle(
                        cell,
                        appLabel,
                        locale,
                        tooltipLabels,
                      )}
                      onMouseEnter={handleCellMouseEnter(cell)}
                      onMouseMove={handleCellMouseMove(cell)}
                      onMouseLeave={() => setTooltip(null)}
                      onFocus={handleCellFocus(cell)}
                      onBlur={() => setTooltip(null)}
                    />
                  ) : (
                    <span
                      key={`${week.index}-${dayIndex}`}
                      className="h-3 w-3 rounded-[3px] border border-transparent"
                    />
                  ),
                ),
              )}
            </div>
          </div>
        </div>
      </div>

      {tooltip &&
        typeof document !== "undefined" &&
        createPortal(
          <div
            role="tooltip"
            className="pointer-events-none fixed z-50 w-56 rounded-md border border-border bg-popover px-3 py-2 text-xs text-popover-foreground shadow-lg"
            style={{ left: tooltip.left, top: tooltip.top }}
          >
            <div className="font-medium">
              {formatCellDate(tooltip.cell, locale)}
            </div>
            <div className="mt-1.5 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
              <span className="text-muted-foreground">{tooltipLabels.app}</span>
              <span className="text-right font-medium">{appLabel}</span>
              <span className="text-muted-foreground">
                {tooltipLabels.tokens}
              </span>
              <span className="text-right font-mono">
                {fmtInt(tooltip.cell.realTotalTokens, locale)}
              </span>
              <span className="text-muted-foreground">
                {tooltipLabels.sessions}
              </span>
              <span className="text-right font-mono">
                {fmtInt(tooltip.cell.sessionCount, locale)}
              </span>
              <span className="text-muted-foreground">
                {tooltipLabels.requests}
              </span>
              <span className="text-right font-mono">
                {fmtInt(tooltip.cell.requestCount, locale)}
              </span>
              <span className="text-muted-foreground">
                {tooltipLabels.cost}
              </span>
              <span className="text-right font-mono">
                {fmtUsd(tooltip.cell.totalCost, 6)}
              </span>
            </div>
          </div>,
          document.body,
        )}
    </div>
  );
}
