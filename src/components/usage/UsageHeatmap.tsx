import { useRef, useState, type KeyboardEvent } from "react";
import { AlertCircle, Flame, Loader2, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useUsageHeatmap } from "@/lib/query/usage";
import { cn } from "@/lib/utils";
import type { UsageHeatmapPoint, UsageRangeSelection } from "@/types/usage";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Tooltip,
  TooltipContent,
  TooltipPortal,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  fmtInt,
  formatTokensShort,
  getLocaleFromLanguage,
  getResolvedLang,
} from "./format";

const HEATMAP_ROWS = 8;
const CELL_SIZE_PX = 12;
const CELL_GAP_PX = 4;
const MAX_AXIS_TICKS = 8;

const LEVEL_CLASSES = [
  "bg-heatmap-0",
  "bg-heatmap-1",
  "bg-heatmap-2",
  "bg-heatmap-3",
  "bg-heatmap-4",
] as const;

export type UsageHeatmapMetric = "requests" | "tokens";

export function getHeatmapIntensityLevel(
  value: number,
  maxValue: number,
): 0 | 1 | 2 | 3 | 4 {
  if (
    !Number.isFinite(value) ||
    !Number.isFinite(maxValue) ||
    value <= 0 ||
    maxValue <= 0
  ) {
    return 0;
  }
  return Math.min(4, Math.max(1, Math.ceil((value / maxValue) * 4))) as
    | 1
    | 2
    | 3
    | 4;
}

function localDateKey(timestamp: number): string {
  const date = new Date(timestamp * 1000);
  return [
    date.getFullYear(),
    String(date.getMonth() + 1).padStart(2, "0"),
    String(date.getDate()).padStart(2, "0"),
  ].join("-");
}

function localTimeLabel(timestamp: number): string {
  const date = new Date(timestamp * 1000);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

function localDateTimeLabel(timestamp: number): string {
  return `${localDateKey(timestamp)} ${localTimeLabel(timestamp)}`;
}

export function formatHeatmapBucketRange(
  bucketStart: number,
  bucketEnd: number,
): string {
  const startDate = localDateKey(bucketStart);
  const endDate = localDateKey(bucketEnd);
  return startDate === endDate
    ? `${localDateTimeLabel(bucketStart)} - ${localTimeLabel(bucketEnd)}`
    : `${localDateTimeLabel(bucketStart)} - ${localDateTimeLabel(bucketEnd)}`;
}

function formatAxisTick(timestamp: number): string {
  const date = new Date(timestamp * 1000);
  return `${String(date.getMonth() + 1).padStart(2, "0")}/${String(date.getDate()).padStart(2, "0")} ${localTimeLabel(timestamp)}`;
}

function getMetricValue(
  point: UsageHeatmapPoint,
  metric: UsageHeatmapMetric,
): number {
  return metric === "requests" ? point.requestCount : point.totalTokens;
}

function getNextCellIndex(
  index: number,
  key: string,
  cellCount: number,
): number {
  if (key === "ArrowUp") {
    return index % HEATMAP_ROWS === 0 ? index : index - 1;
  }
  if (key === "ArrowDown") {
    return index % HEATMAP_ROWS === HEATMAP_ROWS - 1 || index + 1 >= cellCount
      ? index
      : index + 1;
  }
  if (key === "ArrowLeft") {
    return index < HEATMAP_ROWS ? index : index - HEATMAP_ROWS;
  }
  if (key === "ArrowRight") {
    return index + HEATMAP_ROWS >= cellCount ? index : index + HEATMAP_ROWS;
  }
  return index;
}

function buildAxisTicks(points: UsageHeatmapPoint[]) {
  const columnCount = Math.ceil(points.length / HEATMAP_ROWS);
  if (columnCount === 0) return [];

  const tickCount = Math.min(
    MAX_AXIS_TICKS,
    Math.max(1, Math.floor((columnCount - 1) / 6) + 1),
  );
  if (tickCount === 1) {
    return [{ column: 0, timestamp: points[0].bucketStart }];
  }

  const columns = Array.from({ length: tickCount }, (_, index) =>
    Math.round((index * (columnCount - 1)) / (tickCount - 1)),
  );
  return Array.from(new Set(columns)).map((column) => ({
    column,
    timestamp:
      points[Math.min(column * HEATMAP_ROWS, points.length - 1)].bucketStart,
  }));
}

interface UsageHeatmapProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

export function UsageHeatmap({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: UsageHeatmapProps) {
  const { t, i18n } = useTranslation();
  const language = getResolvedLang(i18n);
  const locale = getLocaleFromLanguage(language);
  const [metric, setMetric] = useState<UsageHeatmapMetric>("tokens");
  const [focusedIndex, setFocusedIndex] = useState(0);
  const [openIndex, setOpenIndex] = useState<number | null>(null);
  const cellRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const query = useUsageHeatmap(
    range,
    { appType, providerName, model },
    { refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false },
  );

  const result = query.data;
  const unavailable = result?.status === "detailUnavailable";
  const points = result?.status === "available" ? result.points : [];
  const columnCount = Math.ceil(points.length / HEATMAP_ROWS);
  const gridWidth = Math.max(
    CELL_SIZE_PX,
    columnCount * (CELL_SIZE_PX + CELL_GAP_PX) - CELL_GAP_PX,
  );
  const axisTicks = buildAxisTicks(points);
  const maxValue = Math.max(
    0,
    ...points.map((point) => getMetricValue(point, metric)),
  );
  const metricLabel =
    metric === "requests"
      ? t("usage.heatmap.requests")
      : t("usage.heatmap.tokens");
  const title =
    metric === "requests"
      ? t("usage.heatmap.requestTitle")
      : t("usage.heatmap.tokenTitle");

  const moveFocus = (
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
  ) => {
    if (!event.key.startsWith("Arrow")) return;
    event.preventDefault();
    const nextIndex = getNextCellIndex(index, event.key, points.length);
    setFocusedIndex(nextIndex);
    setOpenIndex(null);
    cellRefs.current[nextIndex]?.focus();
  };

  return (
    <div className="border-t border-border/50 pt-4">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2.5">
          <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-red-500/10">
            <Flame className="h-5 w-5 text-red-500 dark:text-red-400" />
          </div>
          <div className="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-0.5">
            <h3 className="shrink-0 text-base font-semibold sm:text-lg">
              {title}
            </h3>
            {result?.status === "available" && result.bucketMinutes != null && (
              <span className="text-xs leading-tight text-muted-foreground">
                {t("usage.heatmap.bucketMinutes", {
                  minutes: result.bucketMinutes,
                })}
              </span>
            )}
          </div>
        </div>
        <div className="ml-auto flex shrink-0 items-center gap-2">
          <Select
            value={metric}
            onValueChange={(value) => setMetric(value as UsageHeatmapMetric)}
          >
            <SelectTrigger
              className="h-8 w-[150px] text-xs"
              aria-label={t("usage.heatmap.metricLabel")}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="requests">
                {t("usage.heatmap.showRequests")}
              </SelectItem>
              <SelectItem value="tokens">
                {t("usage.heatmap.showTokens")}
              </SelectItem>
            </SelectContent>
          </Select>
          <button
            type="button"
            className="inline-flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50"
            onClick={() => void query.refetch()}
            disabled={query.isFetching || unavailable}
            aria-label={t("usage.heatmap.refresh")}
            title={t("usage.heatmap.refresh")}
          >
            <RefreshCw
              className={cn("h-4 w-4", query.isFetching && "animate-spin")}
            />
          </button>
        </div>
      </div>

      {query.isLoading ? (
        <div
          className="flex h-[176px] items-center justify-center rounded-md bg-muted/20"
          aria-label={t("usage.heatmap.loading")}
        >
          <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
        </div>
      ) : query.isError ? (
        <div className="flex h-[176px] flex-col items-center justify-center gap-3 rounded-md border border-destructive/30 bg-destructive/5 text-center">
          <AlertCircle className="h-5 w-5 text-destructive" />
          <p className="text-sm text-muted-foreground">
            {t("usage.heatmap.error")}
          </p>
          <button
            type="button"
            className="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-muted"
            onClick={() => void query.refetch()}
          >
            {t("usage.heatmap.retry")}
          </button>
        </div>
      ) : unavailable ? (
        <div className="flex h-[176px] flex-col items-center justify-center gap-2 rounded-md border border-border/50 bg-muted/20 px-4 text-center">
          <AlertCircle className="h-5 w-5 text-muted-foreground" />
          <p className="text-sm font-medium">
            {t("usage.heatmap.detailUnavailable")}
          </p>
          {result.availableFrom != null && (
            <p className="text-xs text-muted-foreground">
              {t("usage.heatmap.availableFrom", {
                time: localDateTimeLabel(result.availableFrom),
              })}
            </p>
          )}
        </div>
      ) : (
        <TooltipProvider delayDuration={100}>
          <div
            className="overflow-x-auto pb-2"
            data-testid="usage-heatmap-scroll"
          >
            <div className="mx-auto" style={{ width: gridWidth }}>
              <div
                className="relative mb-2 h-4 text-[10px] text-muted-foreground"
                aria-hidden="true"
              >
                {axisTicks.map((tick, index) => (
                  <span
                    key={tick.column}
                    className={cn(
                      "absolute top-0 whitespace-nowrap",
                      index === 0
                        ? "left-0"
                        : index === axisTicks.length - 1
                          ? "right-0"
                          : "-translate-x-1/2",
                    )}
                    style={
                      index === 0 || index === axisTicks.length - 1
                        ? undefined
                        : {
                            left:
                              tick.column * (CELL_SIZE_PX + CELL_GAP_PX) +
                              CELL_SIZE_PX / 2,
                          }
                    }
                  >
                    {formatAxisTick(tick.timestamp)}
                  </span>
                ))}
              </div>
              <div
                role="grid"
                aria-label={t("usage.heatmap.gridLabel", {
                  metric: metricLabel,
                })}
                className="grid"
                style={{
                  gridAutoFlow: "column",
                  gridTemplateColumns: `repeat(${columnCount}, ${CELL_SIZE_PX}px)`,
                  gridTemplateRows: `repeat(${HEATMAP_ROWS}, ${CELL_SIZE_PX}px)`,
                  gap: CELL_GAP_PX,
                }}
              >
                {points.map((point, index) => {
                  const value = getMetricValue(point, metric);
                  const level = getHeatmapIntensityLevel(value, maxValue);
                  const successRate =
                    point.requestCount > 0
                      ? `${((point.successfulRequests / point.requestCount) * 100).toFixed(1)}%`
                      : "--";
                  const rangeLabel = formatHeatmapBucketRange(
                    point.bucketStart,
                    point.bucketEnd,
                  );
                  const valueLabel =
                    metric === "requests"
                      ? fmtInt(value, locale, "0")
                      : formatTokensShort(value, language);
                  return (
                    <Tooltip
                      key={point.bucketStart}
                      open={openIndex === index}
                      onOpenChange={(open) =>
                        setOpenIndex((current) => {
                          if (open) return index;
                          return current === index ? null : current;
                        })
                      }
                    >
                      <TooltipTrigger asChild>
                        <button
                          ref={(node) => {
                            cellRefs.current[index] = node;
                          }}
                          type="button"
                          role="gridcell"
                          tabIndex={focusedIndex === index ? 0 : -1}
                          aria-label={t("usage.heatmap.cellLabel", {
                            range: rangeLabel,
                            metric: metricLabel,
                            value: valueLabel,
                          })}
                          className={cn(
                            "h-3 w-3 rounded-[3px] ring-1 ring-black/5 transition-transform focus-visible:z-10 focus-visible:scale-125 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring dark:ring-white/10",
                            "hover:z-10 hover:scale-125 hover:ring-2 hover:ring-foreground/20",
                            LEVEL_CLASSES[level],
                          )}
                          onPointerEnter={() => setOpenIndex(index)}
                          onFocus={() => setFocusedIndex(index)}
                          onKeyDown={(event) => moveFocus(event, index)}
                          onClick={() =>
                            setOpenIndex((current) =>
                              current === index ? null : index,
                            )
                          }
                        />
                      </TooltipTrigger>
                      <TooltipPortal>
                        <TooltipContent
                          side="top"
                          sideOffset={8}
                          collisionPadding={16}
                          avoidCollisions
                          sticky="always"
                          hideWhenDetached
                          className="w-60 max-w-[calc(100vw-2rem)] whitespace-normal border border-border bg-popover p-3 text-popover-foreground shadow-lg"
                        >
                          <div className="mb-2 break-words border-b border-border pb-2 font-semibold">
                            {rangeLabel}
                          </div>
                          <dl className="grid grid-cols-2 gap-x-4 gap-y-1.5">
                            <dt className="text-muted-foreground">
                              {t("usage.heatmap.requests")}
                            </dt>
                            <dd className="text-right font-mono">
                              {fmtInt(point.requestCount, locale, "0")}
                            </dd>
                            <dt className="text-muted-foreground">
                              {t("usage.heatmap.successRate")}
                            </dt>
                            <dd className="text-right font-mono">
                              {successRate}
                            </dd>
                            <dt className="text-muted-foreground">
                              {t("usage.heatmap.tokens")}
                            </dt>
                            <dd className="text-right font-mono">
                              {formatTokensShort(point.totalTokens, language)}
                            </dd>
                            <dt className="text-muted-foreground">
                              {t("usage.heatmap.withUsage")}
                            </dt>
                            <dd className="text-right font-mono">
                              {fmtInt(point.requestsWithUsage, locale, "0")}
                            </dd>
                          </dl>
                        </TooltipContent>
                      </TooltipPortal>
                    </Tooltip>
                  );
                })}
              </div>
              <div className="mt-3 flex items-center justify-end gap-1.5 text-xs text-muted-foreground">
                <span>{t("usage.heatmap.less")}</span>
                {LEVEL_CLASSES.map((levelClass, level) => (
                  <span
                    key={level}
                    className={cn(
                      "h-3 w-3 rounded-[3px] ring-1 ring-black/5 dark:ring-white/10",
                      levelClass,
                    )}
                  />
                ))}
                <span>{t("usage.heatmap.more")}</span>
              </div>
            </div>
          </div>
        </TooltipProvider>
      )}
    </div>
  );
}
