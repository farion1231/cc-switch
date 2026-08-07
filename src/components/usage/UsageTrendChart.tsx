import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
} from "react";
import { useTranslation } from "react-i18next";
import {
  Area,
  AreaChart,
  Brush,
  CartesianGrid,
  Legend,
  Line,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import {
  ArrowLeft,
  ArrowRight,
  Loader2,
  Minus,
  Plus,
  RotateCcw,
} from "lucide-react";
import { useUsageTrends } from "@/lib/query/usage";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  fmtInt,
  fmtUsd,
  getLocaleFromLanguage,
  parseFiniteNumber,
} from "./format";
import type {
  TrendGranularity,
  TrendGranularityRequest,
  TrendUnit,
  UsageRangeSelection,
  UsageTrendPoint,
} from "@/types/usage";
import {
  clampDomain,
  domainFromIndexes,
  panDomain,
  zoomDomain,
  type TimeDomain,
} from "./trendViewport";

interface UsageTrendChartProps {
  range: UsageRangeSelection;
  rangeLabel: string;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

const AUTO_GRANULARITY: TrendGranularityRequest = {
  mode: "auto",
  targetPoints: 500,
  maxPoints: 2000,
};

const PRESETS: Array<{
  key: string;
  request: TrendGranularityRequest;
}> = [
  { key: "auto", request: AUTO_GRANULARITY },
  ...([1, 5, 15, 30] as const).map((value) => ({
    key: `${value}-minute`,
    request: { mode: "fixed", value, unit: "minute" } as const,
  })),
  ...([1, 3, 6, 12] as const).map((value) => ({
    key: `${value}-hour`,
    request: { mode: "fixed", value, unit: "hour" } as const,
  })),
  ...([1, 7] as const).map((value) => ({
    key: `${value}-day`,
    request: { mode: "fixed", value, unit: "day" } as const,
  })),
];

interface ChartPoint extends UsageTrendPoint {
  x: number;
  bucketEnd: number;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  cost: number | null;
}

function requestedGranularityKey(request: TrendGranularityRequest): string {
  if (request.mode === "auto") return "auto";
  return `${request.value}-${request.unit}`;
}

function formatGranularity(
  granularity: TrendGranularity,
  t: (key: string, defaultValue: string) => string,
): string {
  return `${granularity.value} ${t(`usage.trendUnit.${granularity.unit}`, granularity.unit)}`;
}

function formatTime(
  timestamp: number,
  unit: TrendUnit,
  locale: string,
): string {
  const options: Intl.DateTimeFormatOptions =
    unit === "day"
      ? { year: "numeric", month: "2-digit", day: "2-digit" }
      : unit === "hour"
        ? { month: "2-digit", day: "2-digit", hour: "2-digit" }
        : {
            month: "2-digit",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
            ...(unit === "second" ? { second: "2-digit" } : {}),
          };
  return new Date(timestamp).toLocaleString(locale, options);
}

function TrendTooltip({
  active,
  payload,
  actualGranularity,
  locale,
  t,
}: {
  active?: boolean;
  payload?: Array<{
    payload: ChartPoint;
    dataKey: string;
    name: string;
    color: string;
    value: number;
  }>;
  actualGranularity?: TrendGranularity;
  locale: string;
  t: (key: string, defaultValue: string) => string;
}) {
  const point = payload?.[0]?.payload;
  if (!active || !point || !actualGranularity) return null;
  const entries = [
    [t("usage.requests", "Requests"), fmtInt(point.requestCount, locale)],
    [t("usage.inputTokens", "Input Tokens"), fmtInt(point.inputTokens, locale)],
    [
      t("usage.outputTokens", "Output Tokens"),
      fmtInt(point.outputTokens, locale),
    ],
    [
      t("usage.cacheCreationTokens", "Cache Write"),
      fmtInt(point.cacheCreationTokens, locale),
    ],
    [
      t("usage.cacheReadTokens", "Cache Hit"),
      fmtInt(point.cacheReadTokens, locale),
    ],
    [t("usage.cost", "Cost"), point.cost == null ? "—" : fmtUsd(point.cost, 6)],
  ];
  return (
    <div className="min-w-64 rounded-lg border bg-background/95 p-3 text-sm shadow-lg backdrop-blur-md">
      <p className="font-medium">
        {formatTime(point.x, actualGranularity.unit, locale)} –{" "}
        {formatTime(point.bucketEnd, actualGranularity.unit, locale)}
      </p>
      <p className="mb-2 text-xs text-muted-foreground">
        {t("usage.actualGranularity", "Actual granularity")}:{" "}
        {formatGranularity(actualGranularity, t)}
      </p>
      <div className="space-y-1">
        {entries.map(([label, value]) => (
          <div key={label} className="flex justify-between gap-6">
            <span className="text-muted-foreground">{label}</span>
            <span className="font-medium tabular-nums">{value}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

export function UsageTrendChart({
  range,
  rangeLabel,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: UsageTrendChartProps) {
  const { t, i18n } = useTranslation();
  const [presetKey, setPresetKey] = useState("auto");
  const [customValue, setCustomValue] = useState(1);
  const [customUnit, setCustomUnit] = useState<TrendUnit>("minute");
  const [granularity, setGranularity] =
    useState<TrendGranularityRequest>(AUTO_GRANULARITY);
  const [visibleDomain, setVisibleDomain] = useState<TimeDomain | null>(null);
  const [dragStart, setDragStart] = useState<{
    x: number;
    domain: TimeDomain;
  } | null>(null);
  const [pinchDistance, setPinchDistance] = useState<number | null>(null);
  const pointers = useRef(new Map<number, number>());
  const chartRef = useRef<HTMLDivElement | null>(null);
  const wheelRef = useRef<
    (event: {
      deltaX: number;
      deltaY: number;
      ctrlKey: boolean;
      clientX: number;
      preventDefault: () => void;
    }) => void
  >(() => {});
  const pendingRef = useRef<
    | { type: "zoom"; factor: number; anchor: number }
    | { type: "pan"; fraction: number }
    | null
  >(null);
  const rafRef = useRef<number | null>(null);
  const zoomRef = useRef<(factor: number, anchor?: number) => void>(() => {});
  const panRef = useRef<(fraction: number) => void>(() => {});
  const flushRef = useRef<() => void>(() => {});
  const wheelHandlerRef = useRef<((event: any) => void) | null>(null);
  // The chart div is conditionally rendered, so a `useEffect([])` would run
  // before the node exists. Use a callback ref to (re)bind the native
  // non-passive wheel listener exactly when the node mounts/unmounts.
  const setChartNode = useCallback((el: HTMLDivElement | null) => {
    const prev = chartRef.current;
    if (prev && wheelHandlerRef.current) {
      prev.removeEventListener("wheel", wheelHandlerRef.current);
    }
    if (rafRef.current != null) {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
    wheelHandlerRef.current = null;
    chartRef.current = el;
    if (el) {
      const handler = (event: any) => wheelRef.current(event);
      wheelHandlerRef.current = handler;
      el.addEventListener("wheel", handler, { passive: false });
    }
  }, []);
  const requestIdentity = `${range.preset}:${range.customStartDate ?? ""}:${range.customEndDate ?? ""}:${range.liveEndTime ?? false}:${appType ?? ""}:${providerName ?? ""}:${model ?? ""}:${requestedGranularityKey(granularity)}`;
  const previousIdentity = useRef(requestIdentity);
  const pendingReset = useRef(false);
  const previousResolution = useRef("");
  const {
    data: response,
    isLoading,
    isFetching,
    isPlaceholderData,
  } = useUsageTrends(range, { appType, providerName, model }, granularity, {
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
  });
  const language = i18n.resolvedLanguage || i18n.language || "en";
  const dateLocale = getLocaleFromLanguage(language);

  const chartData = useMemo<ChartPoint[]>(
    () =>
      response?.data.map((point) => ({
        ...point,
        x: point.bucketStart * 1000,
        bucketEnd: (point.bucketStart + point.bucketSeconds) * 1000,
        inputTokens: point.totalInputTokens,
        outputTokens: point.totalOutputTokens,
        cacheCreationTokens: point.totalCacheCreationTokens,
        cacheReadTokens: point.totalCacheReadTokens,
        cost: parseFiniteNumber(point.totalCost),
      })) ?? [],
    [response],
  );

  // The backend clips the first/last boundary buckets into the query range, so
  // the raw first.x / last.bucketEnd already stay within the user's selection.
  const fullDomain = useMemo<TimeDomain | null>(() => {
    if (chartData.length === 0) return null;
    const first = chartData[0];
    const last = chartData[chartData.length - 1];
    return [first.x, last.bucketEnd];
  }, [chartData]);
  const resolutionIdentity = response
    ? `${response.granularity.value}:${response.granularity.unit}:${response.precision}`
    : "";

  // Track the previous full range so a live refresh can tell whether the user
  // was viewing the whole range (not zoomed/panned) and follow the growing
  // fullDomain instead of clipping new buckets off the right edge.
  const prevFullDomainRef = useRef<TimeDomain | null>(null);
  useEffect(() => {
    if (!fullDomain) {
      setVisibleDomain(null);
      prevFullDomainRef.current = null;
      return;
    }
    const queryChanged = previousIdentity.current !== requestIdentity;
    if (queryChanged) {
      previousIdentity.current = requestIdentity;
      pendingReset.current = true;
    }
    if (isPlaceholderData) return;
    const resolutionChanged =
      previousResolution.current !== "" &&
      previousResolution.current !== resolutionIdentity;
    previousResolution.current = resolutionIdentity;
    const prevFull = prevFullDomainRef.current;
    setVisibleDomain((current) => {
      // If the viewport still equals the previous full range, the user hasn't
      // zoomed or panned, so extend to the new fullDomain (keeps new buckets
      // visible). Otherwise preserve their viewport, clamped to the new bounds.
      const wasAtFull =
        !!prevFull &&
        !!current &&
        current[0] === prevFull[0] &&
        current[1] === prevFull[1];
      const next =
        pendingReset.current || resolutionChanged || !current || wasAtFull
          ? fullDomain
          : clampDomain(current, fullDomain);
      // Return the same ref when values are unchanged so React bails out instead
      // of re-rendering (e.g. a live range's advancing queryEnd that doesn't move
      // a zoomed viewport).
      if (current && next[0] === current[0] && next[1] === current[1]) {
        return current;
      }
      visibleDomainRef.current = next;
      return next;
    });
    prevFullDomainRef.current = fullDomain;
    pendingReset.current = false;
  }, [fullDomain, isPlaceholderData, requestIdentity, resolutionIdentity]);

  const activeDomain = visibleDomain ?? fullDomain;
  // Mirror domains into refs so the rAF flush can compound a zoom/pan factor
  // against the current domain without waiting for React's async state commit.
  // Without this, consecutive pinch events apply to a stale base and the
  // gesture stutters or looks inert.
  const fullDomainRef = useRef<TimeDomain | null>(null);
  fullDomainRef.current = fullDomain;
  const visibleDomainRef = useRef<TimeDomain | null>(null);
  visibleDomainRef.current = activeDomain;
  const commitDomain = useCallback((next: TimeDomain) => {
    const full = fullDomainRef.current;
    if (!full) return;
    const clamped = clampDomain(next, full);
    visibleDomainRef.current = clamped;
    setVisibleDomain(clamped);
  }, []);
  const updateDomain = useCallback(
    (next: TimeDomain) => commitDomain(next),
    [commitDomain],
  );
  const zoom = useCallback(
    (factor: number, anchor = 0.5) => {
      const domain = visibleDomainRef.current;
      const full = fullDomainRef.current;
      if (!domain || !full) return;
      commitDomain(zoomDomain(domain, full, factor, anchor));
    },
    [commitDomain],
  );
  const pan = useCallback(
    (fraction: number) => {
      const domain = visibleDomainRef.current;
      const full = fullDomainRef.current;
      if (!domain || !full) return;
      commitDomain(panDomain(domain, full, fraction));
    },
    [commitDomain],
  );
  const reset = useCallback(() => {
    const full = fullDomainRef.current;
    if (!full) return;
    visibleDomainRef.current = full;
    setVisibleDomain(full);
  }, []);

  // WebKit fires non-standard gesturestart/gesturechange/gestureend for trackpad
  // pinch. Each gesturechange carries a cumulative `scale` (1.0 = none, >1 =
  // fingers apart). This is the reliable pinch channel on macOS WKWebView: the
  // ctrl+wheel synthesis there collapses a whole pinch to a single near-zero
  // wheel event, so wheel can't drive pinch here. A transparent overlay (see
  // JSX) is the gesture target instead of the Recharts <path> -- React never
  // recreates the overlay, so the gesture survives the per-frame Recharts
  // re-render that would otherwise destroy the target path and cancel the pinch.
  const gestureStartDomainRef = useRef<TimeDomain | null>(null);
  const gestureAnchorRef = useRef(0.5);
  const gestureActiveRef = useRef(false);
  useEffect(() => {
    const inChart = (e: any) => {
      const node = chartRef.current;
      const t = e.target as Node | null;
      return !!node && !!t && node.contains(t);
    };
    const onStart = (e: any) => {
      if (!inChart(e)) return;
      const full = fullDomainRef.current;
      const start = visibleDomainRef.current;
      if (!full || !start) return;
      const rect = chartRef.current!.getBoundingClientRect();
      const cx = typeof e.clientX === "number" ? e.clientX : null;
      gestureAnchorRef.current =
        cx != null && rect.width > 0 ? (cx - rect.left) / rect.width : 0.5;
      gestureStartDomainRef.current = start;
      gestureActiveRef.current = true;
      e.preventDefault?.();
    };
    const onEnd = () => {
      gestureActiveRef.current = false;
    };
    const onChange = (e: any) => {
      if (!gestureActiveRef.current) return;
      const full = fullDomainRef.current;
      const start = gestureStartDomainRef.current;
      if (!full || !start) return;
      e.preventDefault?.();
      // `scale` is cumulative from gesturestart, so re-derive the target from the
      // START domain each frame (no compounding drift, no sensitivity to lost
      // events). Invert (1/scale): fingers apart -> scale>1 -> shrink span -> zoom
      // in, matching zoom(factor<1)=in used by the toolbar/keyboard/touch paths.
      const target = zoomDomain(
        start,
        full,
        1 / e.scale,
        gestureAnchorRef.current,
      );
      const clamped = clampDomain(target, full);
      visibleDomainRef.current = clamped;
      setVisibleDomain(clamped);
    };
    window.addEventListener("gesturestart", onStart, { capture: true });
    window.addEventListener("gesturechange", onChange, { capture: true });
    window.addEventListener("gestureend", onEnd, { capture: true });
    return () => {
      window.removeEventListener("gesturestart", onStart, { capture: true });
      window.removeEventListener("gesturechange", onChange, { capture: true });
      window.removeEventListener("gestureend", onEnd, { capture: true });
    };
  }, []);

  const handlePreset = (value: string) => {
    setPresetKey(value);
    if (value === "custom") {
      setGranularity({ mode: "fixed", value: customValue, unit: customUnit });
      return;
    }
    const preset = PRESETS.find((item) => item.key === value);
    if (preset) setGranularity(preset.request);
  };
  const applyCustom = (value = customValue, unit = customUnit) => {
    const safeValue = Math.max(1, Math.trunc(value || 1));
    setCustomValue(safeValue);
    setCustomUnit(unit);
    setPresetKey("custom");
    setGranularity({ mode: "fixed", value: safeValue, unit });
  };

  // Native non-passive wheel listener (React's onWheel is passive, so
  // preventDefault is a no-op there). Routing, matching macOS habits:
  //  - ctrlKey + wheel -> pinch zoom. On WebKit the gesture* channel owns
  //    trackpad pinch, so while a gesture is active these spurious wheel events
  //    are swallowed (no double-zoom). Otherwise this is the pinch channel
  //    (mouse ctrl+wheel, or non-WebKit browsers where pinch synthesizes wheel).
  //  - horizontal wheel (deltaX)  -> pan the time window (preventDefault)
  //  - vertical wheel, no ctrlKey -> leave to the page (no preventDefault)
  // Wheel pinch fires many small events per frame; accumulate the factor and
  // apply once per animation frame so it stays smooth under Recharts redraw.
  zoomRef.current = zoom;
  panRef.current = pan;
  flushRef.current = () => {
    rafRef.current = null;
    const pending = pendingRef.current;
    pendingRef.current = null;
    if (!pending) return;
    if (pending.type === "zoom")
      zoomRef.current(pending.factor, pending.anchor);
    else panRef.current(pending.fraction);
  };
  wheelRef.current = (event) => {
    if (!activeDomain || !fullDomain) return;
    if (event.ctrlKey) {
      event.preventDefault();
      // On WebKit, trackpad pinch is owned by gesture*; swallow the spurious
      // ctrl+wheel events while a gesture is active so they don't double-zoom.
      if (gestureActiveRef.current) return;
      const rect = chartRef.current?.getBoundingClientRect();
      const anchor =
        rect && rect.width > 0 ? (event.clientX - rect.left) / rect.width : 0.5;
      // Pinch out (zoom in) yields negative deltaY -> factor<1 -> zoom in,
      // consistent with zoom(factor<1)=in used by the gesture/toolbar/touch paths.
      const factor = Math.exp(event.deltaY * 0.01);
      const pending = pendingRef.current;
      if (pending && pending.type === "zoom") pending.factor *= factor;
      else pendingRef.current = { type: "zoom", factor, anchor };
      if (rafRef.current == null)
        rafRef.current = requestAnimationFrame(() => flushRef.current());
      return;
    }
    if (
      event.deltaX !== 0 &&
      Math.abs(event.deltaX) >= Math.abs(event.deltaY)
    ) {
      event.preventDefault();
      const rect = chartRef.current?.getBoundingClientRect();
      const width = rect?.width ?? 1;
      // Natural-scroll: two fingers right -> view follows right (pan right).
      const fraction = event.deltaX / width;
      const pending = pendingRef.current;
      if (pending && pending.type === "pan") pending.fraction += fraction;
      else pendingRef.current = { type: "pan", fraction };
      if (rafRef.current == null)
        rafRef.current = requestAnimationFrame(() => flushRef.current());
      return;
    }
    // vertical scroll, no ctrlKey: no preventDefault -> page scrolls naturally
  };
  const handlePointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (!activeDomain) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    pointers.current.set(event.pointerId, event.clientX);
    if (pointers.current.size === 1) {
      setDragStart({ x: event.clientX, domain: activeDomain });
    } else if (pointers.current.size === 2) {
      const values = [...pointers.current.values()];
      setPinchDistance(Math.abs(values[1] - values[0]));
      setDragStart(null);
    }
  };
  const handlePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (!fullDomain) return;
    if (pointers.current.has(event.pointerId))
      pointers.current.set(event.pointerId, event.clientX);
    if (pointers.current.size === 2 && pinchDistance) {
      const values = [...pointers.current.values()];
      const distance = Math.abs(values[1] - values[0]);
      if (distance > 0) {
        zoom(pinchDistance / distance, 0.5);
        setPinchDistance(distance);
      }
      return;
    }
    if (!dragStart) return;
    const width = event.currentTarget.getBoundingClientRect().width;
    if (width <= 0) return;
    const delta =
      ((dragStart.x - event.clientX) / width) *
      (dragStart.domain[1] - dragStart.domain[0]);
    updateDomain([dragStart.domain[0] + delta, dragStart.domain[1] + delta]);
  };
  const handlePointerUp = (event: PointerEvent<HTMLDivElement>) => {
    pointers.current.delete(event.pointerId);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    if (pointers.current.size < 2) setPinchDistance(null);
    if (pointers.current.size === 0) setDragStart(null);
  };
  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    let handled = true;
    switch (event.key) {
      case "+":
      case "=":
        zoom(0.8);
        break;
      case "-":
        zoom(1.25);
        break;
      case "ArrowLeft":
        pan(-0.2);
        break;
      case "ArrowRight":
        pan(0.2);
        break;
      case "Home":
        if (activeDomain && fullDomain)
          updateDomain([
            fullDomain[0],
            fullDomain[0] + (activeDomain[1] - activeDomain[0]),
          ]);
        break;
      case "End":
        if (activeDomain && fullDomain)
          updateDomain([
            fullDomain[1] - (activeDomain[1] - activeDomain[0]),
            fullDomain[1],
          ]);
        break;
      case "0":
        reset();
        break;
      default:
        handled = false;
    }
    if (handled) event.preventDefault();
  };

  const brushIndexes = useMemo(() => {
    if (!activeDomain || chartData.length === 0)
      return { startIndex: 0, endIndex: 0 };
    const startIndex = Math.max(
      0,
      chartData.findIndex((point) => point.bucketEnd > activeDomain[0]),
    );
    let endIndex = chartData.length - 1;
    while (endIndex > startIndex && chartData[endIndex].x >= activeDomain[1]) {
      endIndex -= 1;
    }
    return { startIndex, endIndex };
  }, [activeDomain, chartData]);

  if (isLoading && !response) {
    return (
      <div className="flex h-[350px] items-center justify-center rounded-xl border border-border/50 bg-card/40">
        <Loader2 className="h-8 w-8 animate-spin text-muted-foreground/30" />
      </div>
    );
  }

  const actual = response?.granularity;
  const actualKey = actual ? `${actual.value}-${actual.unit}` : "";
  const adjusted =
    granularity.mode === "fixed" &&
    actualKey !== `${granularity.value}-${granularity.unit}`;
  const tickFormatter = (value: number) =>
    actual ? formatTime(value, actual.unit, dateLocale) : "";
  const tooltip = (
    <TrendTooltip
      actualGranularity={actual}
      locale={dateLocale}
      t={(key, fallback) => t(key, fallback)}
    />
  );
  const ariaStatus =
    activeDomain && actual
      ? `${formatGranularity(actual, (key, fallback) => t(key, fallback))}; ${formatTime(activeDomain[0], actual.unit, dateLocale)} – ${formatTime(activeDomain[1], actual.unit, dateLocale)}`
      : t("usage.noData", "No data");

  return (
    <div className="rounded-xl border border-border/50 bg-card/40 p-4 backdrop-blur-sm sm:p-6">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div>
          <h3 className="text-lg font-semibold">
            {t("usage.trends", "Usage trends")}
          </h3>
          <p className="text-sm text-muted-foreground">{rangeLabel}</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Select value={presetKey} onValueChange={handlePreset}>
            <SelectTrigger
              className="h-8 w-[150px]"
              aria-label={t("usage.trendGranularity", "Trend granularity")}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="auto">
                {t("usage.granularityAuto", "Auto")}
              </SelectItem>
              {PRESETS.filter((item) => item.key !== "auto").map((item) => {
                const request = item.request as Extract<
                  TrendGranularityRequest,
                  { mode: "fixed" }
                >;
                return (
                  <SelectItem key={item.key} value={item.key}>
                    {request.value}{" "}
                    {t(`usage.trendUnit.${request.unit}`, request.unit)}
                  </SelectItem>
                );
              })}
              <SelectItem value="custom">
                {t("usage.customGranularity", "Custom")}
              </SelectItem>
            </SelectContent>
          </Select>
          {presetKey === "custom" && (
            <>
              <Input
                className="h-8 w-20"
                type="number"
                min={1}
                value={customValue}
                onChange={(event) =>
                  applyCustom(Number(event.target.value), customUnit)
                }
                aria-label={t("usage.granularityValue", "Granularity value")}
              />
              <Select
                value={customUnit}
                onValueChange={(value) =>
                  applyCustom(customValue, value as TrendUnit)
                }
              >
                <SelectTrigger
                  className="h-8 w-[110px]"
                  aria-label={t("usage.granularityUnit", "Granularity unit")}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {(["second", "minute", "hour", "day"] as TrendUnit[]).map(
                    (unit) => (
                      <SelectItem key={unit} value={unit}>
                        {t(`usage.trendUnit.${unit}`, unit)}
                      </SelectItem>
                    ),
                  )}
                </SelectContent>
              </Select>
            </>
          )}
          {actual && (
            <Badge variant="outline">
              {formatGranularity(actual, (key, fallback) => t(key, fallback))}
            </Badge>
          )}
          {isFetching && (
            <Loader2
              className="h-4 w-4 animate-spin text-muted-foreground"
              aria-label={t("usage.refreshing", "Refreshing")}
            />
          )}
        </div>
      </div>

      {(response?.precision === "daily" || adjusted) && (
        <div className="mb-3 rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-800 dark:text-amber-200">
          {response?.precision === "daily"
            ? t(
                "usage.dailyPrecisionNotice",
                "Older history only retains calendar-day summaries; sub-day detail cannot be restored.",
              )
            : t(
                "usage.granularityAdjusted",
                "The server increased the granularity to keep the chart responsive.",
              )}
        </div>
      )}

      <div
        className="mb-2 flex flex-wrap justify-end gap-1"
        role="toolbar"
        aria-label={t("usage.zoomControls", "Trend zoom controls")}
      >
        <Button
          size="icon"
          variant="ghost"
          onClick={() => zoom(0.8)}
          aria-label={t("usage.zoomIn", "Zoom in")}
        >
          <Plus className="h-4 w-4" />
        </Button>
        <Button
          size="icon"
          variant="ghost"
          onClick={() => zoom(1.25)}
          aria-label={t("usage.zoomOut", "Zoom out")}
        >
          <Minus className="h-4 w-4" />
        </Button>
        <Button
          size="icon"
          variant="ghost"
          onClick={() => pan(-0.2)}
          aria-label={t("usage.panLeft", "Pan left")}
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <Button
          size="icon"
          variant="ghost"
          onClick={() => pan(0.2)}
          aria-label={t("usage.panRight", "Pan right")}
        >
          <ArrowRight className="h-4 w-4" />
        </Button>
        <Button
          size="icon"
          variant="ghost"
          onClick={reset}
          aria-label={t("usage.resetZoom", "Reset zoom")}
        >
          <RotateCcw className="h-4 w-4" />
        </Button>
      </div>

      {chartData.length === 0 || !activeDomain ? (
        <div className="flex h-[300px] items-center justify-center text-sm text-muted-foreground">
          {t("usage.noData", "No data")}
        </div>
      ) : (
        <div
          ref={setChartNode}
          className={`select-none outline-none ${dragStart ? "cursor-grabbing" : "cursor-grab"}`}
          style={{ touchAction: "none" }}
          tabIndex={0}
          role="application"
          aria-label={t(
            "usage.trendChartAria",
            "Interactive usage trend chart. Use plus and minus to zoom, arrow keys to pan, and zero to reset.",
          )}
          onKeyDown={handleKeyDown}
          onPointerDown={handlePointerDown}
          onPointerMove={handlePointerMove}
          onPointerUp={handlePointerUp}
          onPointerCancel={handlePointerUp}
        >
          <div className="relative h-[360px] w-full">
            {/* Stable gesture target. This transparent overlay sits above the
                Recharts SVG and is the element under the cursor, so WebKit's
                trackpad pinch targets it. React never recreates this div, so the
                gesture survives the per-frame Recharts re-renders that would
                otherwise destroy the <path> target and cancel the pinch. It
                intercepts the mouse events Recharts' tooltip needs, so we
                re-dispatch them on the underlying SVG to keep tooltips working. */}
            <div
              className="trend-gesture-overlay absolute inset-0 z-10"
              aria-hidden="true"
              onMouseMove={(e) => {
                const svg = chartRef.current?.querySelector("svg");
                svg?.dispatchEvent(
                  new MouseEvent("mousemove", {
                    clientX: e.clientX,
                    clientY: e.clientY,
                    bubbles: true,
                    cancelable: true,
                  }),
                );
              }}
              onMouseLeave={() => {
                const svg = chartRef.current?.querySelector("svg");
                svg?.dispatchEvent(
                  new MouseEvent("mouseleave", {
                    bubbles: true,
                    cancelable: true,
                  }),
                );
              }}
            />
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart
                data={chartData}
                margin={{ top: 10, right: 28, left: 0, bottom: 8 }}
              >
                <defs>
                  {[
                    ["Input", "#3b82f6"],
                    ["Output", "#22c55e"],
                    ["CacheCreation", "#f97316"],
                    ["CacheRead", "#a855f7"],
                  ].map(([name, color]) => (
                    <linearGradient
                      key={name}
                      id={`trend${name}`}
                      x1="0"
                      y1="0"
                      x2="0"
                      y2="1"
                    >
                      <stop offset="5%" stopColor={color} stopOpacity={0.2} />
                      <stop offset="95%" stopColor={color} stopOpacity={0} />
                    </linearGradient>
                  ))}
                </defs>
                <CartesianGrid
                  strokeDasharray="3 3"
                  vertical={false}
                  stroke="hsl(var(--border))"
                  opacity={0.4}
                />
                <XAxis
                  type="number"
                  dataKey="x"
                  domain={activeDomain}
                  allowDataOverflow
                  axisLine={false}
                  tickLine={false}
                  tickFormatter={tickFormatter}
                  tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 11 }}
                />
                <YAxis
                  yAxisId="tokens"
                  axisLine={false}
                  tickLine={false}
                  tickFormatter={(value) => fmtInt(value, dateLocale)}
                  tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 11 }}
                />
                <YAxis
                  yAxisId="cost"
                  orientation="right"
                  axisLine={false}
                  tickLine={false}
                  tickFormatter={(value) => fmtUsd(value, 4)}
                  tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 11 }}
                />
                <Tooltip
                  content={tooltip}
                  cursor={{
                    stroke: "hsl(var(--foreground))",
                    strokeOpacity: 0.25,
                  }}
                />
                <Legend />
                <Area
                  yAxisId="tokens"
                  type="monotone"
                  dataKey="inputTokens"
                  name={t("usage.inputTokens", "Input Tokens")}
                  stroke="#3b82f6"
                  fill="url(#trendInput)"
                  strokeWidth={2}
                  isAnimationActive={false}
                />
                <Area
                  yAxisId="tokens"
                  type="monotone"
                  dataKey="outputTokens"
                  name={t("usage.outputTokens", "Output Tokens")}
                  stroke="#22c55e"
                  fill="url(#trendOutput)"
                  strokeWidth={2}
                  isAnimationActive={false}
                />
                <Area
                  yAxisId="tokens"
                  type="monotone"
                  dataKey="cacheCreationTokens"
                  name={t("usage.cacheCreationTokens", "Cache Write")}
                  stroke="#f97316"
                  fill="url(#trendCacheCreation)"
                  strokeWidth={2}
                  isAnimationActive={false}
                />
                <Area
                  yAxisId="tokens"
                  type="monotone"
                  dataKey="cacheReadTokens"
                  name={t("usage.cacheReadTokens", "Cache Hit")}
                  stroke="#a855f7"
                  fill="url(#trendCacheRead)"
                  strokeWidth={2}
                  isAnimationActive={false}
                />
                <Line
                  yAxisId="cost"
                  type="monotone"
                  dataKey="cost"
                  name={t("usage.cost", "Cost")}
                  stroke="#f43f5e"
                  strokeWidth={2}
                  dot={false}
                  connectNulls
                  isAnimationActive={false}
                />
              </AreaChart>
            </ResponsiveContainer>
          </div>
          <div
            className="h-[64px] w-full"
            onPointerDown={(event) => event.stopPropagation()}
          >
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart
                data={chartData}
                margin={{ top: 0, right: 16, left: 0, bottom: 0 }}
              >
                <XAxis type="number" dataKey="x" hide />
                <Area
                  dataKey="totalTokens"
                  stroke="hsl(var(--muted-foreground))"
                  fill="hsl(var(--muted))"
                  isAnimationActive={false}
                />
                <Brush
                  dataKey="x"
                  height={28}
                  travellerWidth={10}
                  startIndex={brushIndexes.startIndex}
                  endIndex={brushIndexes.endIndex}
                  tickFormatter={tickFormatter}
                  onChange={(range) => {
                    if (range.startIndex == null || range.endIndex == null)
                      return;
                    const next = domainFromIndexes(
                      chartData.map((point) => point.x),
                      range.startIndex,
                      range.endIndex,
                      chartData.map((point) => point.bucketSeconds),
                    );
                    if (next) updateDomain(next);
                  }}
                />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        </div>
      )}
      <div className="sr-only" aria-live="polite">
        {ariaStatus}
      </div>
    </div>
  );
}
