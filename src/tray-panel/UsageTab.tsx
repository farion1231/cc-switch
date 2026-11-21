import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { AnimatePresence, LayoutGroup, motion } from "framer-motion";
import {
  Activity,
  Check,
  Coins,
  Cpu,
  DatabaseZap,
  Loader2,
  SlidersHorizontal,
  X,
} from "lucide-react";
import { usageApi } from "@/lib/api";
import type { AppId } from "@/lib/api/types";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { useSettingsQuery } from "@/lib/query";
import { resolveUsageRange, getUsageRangePresetLabel } from "@/lib/usageRange";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import {
  fmtInt,
  formatHitRatePercent,
  formatTokensCompact,
  getLocaleFromLanguage,
  parseFiniteNumber,
  resolveTrendChartType,
} from "@/components/usage/format";
import {
  buildUsageTrendChartData,
  createUsageTrendTokenTickFormatter,
  hitRateAxisFloor,
  type UsageTrendChartPoint,
} from "@/components/usage/UsageTrendChart";
import type {
  DailyStats,
  ModelStats,
  ProviderStats,
  UsageSummary,
  UsageSummaryByApp,
} from "@/types/usage";
import { cn } from "@/lib/utils";
import { ClassicUsageTab } from "./ClassicUsageTab";
import {
  AppFilterStrip,
  HitRateRing,
  MetricBentoCard,
  MiniCostBars,
  MiniRequestsSpark,
  Segmented,
  StackedTokenBar,
  TOKEN_SEGMENTS,
  UsageCardDetailView,
  UsageTrendGraphic,
  type BreakdownDimension,
  type BreakdownItem,
  type DetailSubMetric,
  type TrayUsagePoint,
  type UsageCardId,
} from "./usageWidgets";

const RANGES = ["today", "7d", "30d", "all"] as const;
type Range = (typeof RANGES)[number];
const METRICS: readonly UsageCardId[] = [
  "tokens",
  "requests",
  "cost",
  "hitRate",
];

const RANGE_KEY = "cc-switch-tray-usage-range";
const METRIC_KEY = "cc-switch-tray-usage-metric";
/** 柱子最多这么多根，再多就把相邻的几天并成一根 */
const MAX_BARS = 31;
const DAY_SECONDS = 24 * 60 * 60;

function remembered<T extends string>(
  key: string,
  options: readonly T[],
  fallback: T,
): T {
  try {
    const value = window.localStorage.getItem(key);
    if (value && (options as readonly string[]).includes(value)) {
      return value as T;
    }
  } catch {
    // 读不到用默认值
  }
  return fallback;
}

function remember(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // 记不住也不影响使用
  }
}

interface UsageScope {
  appType?: string;
  providerName?: string;
  model?: string;
}

interface UsageData {
  summary: UsageSummary | null;
  trends: DailyStats[];
  startDate: number;
  endDate: number;
}

async function resolveRangeBounds(
  range: Range,
  scope: UsageScope,
): Promise<{ startDate: number; endDate: number; emptyAll: boolean }> {
  const resolved = resolveUsageRange({ preset: range });
  let { startDate } = resolved;
  const { endDate } = resolved;
  if (range === "all") {
    const first = await usageApi.getUsageFirstDate(
      scope.appType,
      scope.providerName,
      scope.model,
    );
    if (!first) return { startDate, endDate, emptyAll: true };
    const [y, m, d] = first.split("-").map(Number);
    startDate = Math.floor(new Date(y, m - 1, d).getTime() / 1000);
    // 不到一天时后端会按小时分桶，至少给满一天按天显示
    startDate = Math.min(startDate, endDate - DAY_SECONDS - 1);
  }
  return { startDate, endDate, emptyAll: false };
}

/** 「全部」从最早有记录的那天算起（按 0 起算后端会按天铺满几十年）。 */
async function loadUsage(range: Range, scope: UsageScope): Promise<UsageData> {
  const { startDate, endDate, emptyAll } = await resolveRangeBounds(
    range,
    scope,
  );
  if (emptyAll) return { summary: null, trends: [], startDate, endDate };
  const [summary, trends] = await Promise.all([
    usageApi.getUsageSummary(
      startDate,
      endDate,
      scope.appType,
      scope.providerName,
      scope.model,
    ),
    usageApi.getUsageTrends(
      startDate,
      endDate,
      scope.appType,
      scope.providerName,
      scope.model,
    ),
  ]);
  return { summary, trends, startDate, endDate };
}

interface DimensionData {
  byApp: UsageSummaryByApp[];
  providers: ProviderStats[];
  models: ModelStats[];
}

async function loadDimensions(
  range: Range,
  scope: UsageScope,
): Promise<DimensionData> {
  const { startDate, endDate, emptyAll } = await resolveRangeBounds(range, {});
  if (emptyAll) return { byApp: [], providers: [], models: [] };
  const [byApp, providers, models] = await Promise.all([
    usageApi.getUsageSummaryByApp
      ? usageApi.getUsageSummaryByApp(
          startDate,
          endDate,
          scope.providerName,
          scope.model,
        )
      : Promise.resolve([]),
    usageApi.getProviderStats
      ? usageApi.getProviderStats(
          startDate,
          endDate,
          scope.appType,
          undefined,
          scope.model,
        )
      : Promise.resolve([]),
    usageApi.getModelStats
      ? usageApi.getModelStats(
          startDate,
          endDate,
          scope.appType,
          scope.providerName,
          undefined,
        )
      : Promise.resolve([]),
  ]);
  return { byApp, providers, models };
}

/** 相邻几根并成一根，标签用第一根的。 */
function bucketize(
  points: UsageTrendChartPoint[],
  maxBars: number,
): UsageTrendChartPoint[] {
  if (points.length <= maxBars) return points;
  const size = Math.ceil(points.length / maxBars);
  const merged: UsageTrendChartPoint[] = [];
  for (let i = 0; i < points.length; i += size) {
    const chunk = points.slice(i, i + size);
    const first = chunk[0];
    const last = chunk[chunk.length - 1];
    const inputTokens = chunk.reduce((sum, p) => sum + p.inputTokens, 0);
    const outputTokens = chunk.reduce((sum, p) => sum + p.outputTokens, 0);
    const cacheCreationTokens = chunk.reduce(
      (sum, p) => sum + p.cacheCreationTokens,
      0,
    );
    const cacheReadTokens = chunk.reduce(
      (sum, p) => sum + p.cacheReadTokens,
      0,
    );
    const cacheable = inputTokens + cacheCreationTokens + cacheReadTokens;
    merged.push({
      ...first,
      tooltipLabel: `${first.label} – ${last.label}`,
      inputTokens,
      outputTokens,
      cacheCreationTokens,
      cacheReadTokens,
      requests: chunk.reduce((sum, p) => sum + p.requests, 0),
      tokens: chunk.reduce((sum, p) => sum + p.tokens, 0),
      cost: chunk.some((p) => p.cost != null)
        ? chunk.reduce((sum, p) => sum + (p.cost ?? 0), 0)
        : null,
      hitRate: cacheable > 0 ? cacheReadTokens / cacheable : null,
    });
  }
  return merged;
}

function appDisplayName(appType: string): string {
  return APP_DISPLAY_NAME[appType as AppId] ?? appType;
}

function metricValueOfPoint(point: TrayUsagePoint, card: UsageCardId): number {
  if (card === "cost") return point.cost ?? 0;
  if (card === "requests") return point.requests;
  if (card === "hitRate") return point.hitRate ?? 0;
  return point.tokens;
}

export function UsageTab({
  refreshKey,
  visible,
  platform = "windows",
}: {
  /** 每次弹出变一次：重新取数据 */
  refreshKey: number;
  visible: boolean;
  /** 仅在 Windows 和 macOS 使用新版设计样式；Linux 等维持改动前样式 */
  platform?: string;
}) {
  if (platform !== "windows" && platform !== "macos") {
    return <ClassicUsageTab refreshKey={refreshKey} visible={visible} />;
  }
  return <EnhancedUsageTab refreshKey={refreshKey} visible={visible} />;
}

function EnhancedUsageTab({
  refreshKey,
  visible,
}: {
  refreshKey: number;
  visible: boolean;
}) {
  const { t, i18n } = useTranslation();
  const { money, moneyTick, rate } = useMoneyFormat();
  // 柱形图 / 曲线面积图跟主窗口用量「设置」里选的一样
  const { data: settings } = useSettingsQuery();
  const chartType = resolveTrendChartType(settings?.usageDisplay);

  const [range, setRange] = useState<Range>(() =>
    remembered(RANGE_KEY, RANGES, "today"),
  );
  const [metric, setMetric] = useState<UsageCardId>(() =>
    remembered(METRIC_KEY, METRICS, "tokens"),
  );
  const [appFilter, setAppFilter] = useState<string | undefined>(undefined);
  const [providerFilter, setProviderFilter] = useState<string | undefined>(
    undefined,
  );
  const [modelFilter, setModelFilter] = useState<string | undefined>(undefined);
  const [selectedXKey, setSelectedXKey] = useState<string | null>(null);
  const [detailCard, setDetailCard] = useState<UsageCardId | null>(null);
  const [dimension, setDimension] = useState<BreakdownDimension>("app");
  const [filterMenuOpen, setFilterMenuOpen] = useState(false);

  const filterPopoverRef = useRef<HTMLDivElement>(null);
  const prevRefreshKey = useRef(refreshKey);

  // 每次重新弹出托盘面板时收起下钻和筛选弹层，保持干净起点
  useEffect(() => {
    if (prevRefreshKey.current !== refreshKey) {
      prevRefreshKey.current = refreshKey;
      setDetailCard(null);
      setFilterMenuOpen(false);
      setSelectedXKey(null);
    }
  }, [refreshKey]);

  // Esc 逐层退出：筛选弹层 -> 单卡下钻 -> 已选时段；都不在时交还给 TrayPanel 关面板
  useEffect(() => {
    if (!visible) return;
    if (!filterMenuOpen && detailCard === null && selectedXKey === null) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      if (filterMenuOpen) setFilterMenuOpen(false);
      else if (detailCard !== null) setDetailCard(null);
      else if (selectedXKey !== null) setSelectedXKey(null);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [visible, filterMenuOpen, detailCard, selectedXKey]);

  // 点击外部关闭筛选浮层
  useEffect(() => {
    if (!filterMenuOpen) return;
    const onPointerDown = (event: MouseEvent) => {
      if (
        filterPopoverRef.current &&
        !filterPopoverRef.current.contains(event.target as Node)
      ) {
        setFilterMenuOpen(false);
      }
    };
    window.addEventListener("mousedown", onPointerDown);
    return () => window.removeEventListener("mousedown", onPointerDown);
  }, [filterMenuOpen]);

  const locale = getLocaleFromLanguage(
    i18n.resolvedLanguage || i18n.language || "en",
  );
  const scope: UsageScope = useMemo(
    () => ({
      appType: appFilter,
      providerName: providerFilter,
      model: modelFilter,
    }),
    [appFilter, providerFilter, modelFilter],
  );

  const { data, isLoading } = useQuery({
    queryKey: [
      "tray-panel-usage",
      range,
      appFilter ?? null,
      providerFilter ?? null,
      modelFilter ?? null,
      refreshKey,
    ],
    queryFn: () => loadUsage(range, scope),
    enabled: visible,
    refetchInterval: visible ? 60_000 : false,
    placeholderData: (previous) => previous,
  });

  const { data: dimData } = useQuery({
    queryKey: [
      "tray-panel-usage-dims",
      range,
      appFilter ?? null,
      providerFilter ?? null,
      modelFilter ?? null,
      refreshKey,
    ],
    queryFn: () => loadDimensions(range, scope),
    enabled: visible,
    refetchInterval: visible ? 60_000 : false,
    placeholderData: (previous) => previous,
  });

  const tickFormatter = useMemo(
    () => createUsageTrendTokenTickFormatter(locale),
    [locale],
  );

  const chartData: TrayUsagePoint[] = useMemo(() => {
    if (!data) return [];
    const points = buildUsageTrendChartData(data.trends, {
      isHourly: data.endDate - data.startDate <= DAY_SECONDS,
      dateLocale: locale,
      startDate: data.startDate,
      endDate: data.endDate,
    });
    return bucketize(points, MAX_BARS).map((point) => ({
      ...point,
      // 金额先换算成显示币种，刻度才整齐
      costShown: point.cost == null ? null : point.cost * rate,
    }));
  }, [data, locale, rate]);

  // 范围或过滤变化导致已选时段不存在时自动清空
  const selectedPoint = useMemo(
    () =>
      selectedXKey
        ? (chartData.find((p) => p.xKey === selectedXKey) ?? null)
        : null,
    [chartData, selectedXKey],
  );

  const summary = data?.summary;
  const currencySymbol = moneyTick(0).replace(/[\d.,\s-]/g, "");
  const rangeLabel = getUsageRangePresetLabel(range, t);

  const hitRateFloor = useMemo(
    () => hitRateAxisFloor(chartData.map((point) => point.hitRate)),
    [chartData],
  );

  const formatMetricValue = (point: TrayUsagePoint, target: UsageCardId) => {
    if (target === "cost") return money(point.cost, 4);
    if (target === "requests") return fmtInt(point.requests, locale);
    if (target === "hitRate") {
      return point.hitRate == null ? "—" : formatHitRatePercent(point.hitRate);
    }
    return formatTokensCompact(point.tokens, locale);
  };

  // 当前展示的汇总口径：如果联动选中了图表上的某根柱/点，卡片即刻切换为该时段数值
  const activeCost = selectedPoint
    ? selectedPoint.cost
    : summary
      ? parseFiniteNumber(summary.totalCost)
      : null;
  const activeRequests = selectedPoint
    ? selectedPoint.requests
    : (summary?.totalRequests ?? 0);
  const activeInput = selectedPoint
    ? selectedPoint.inputTokens
    : (summary?.totalInputTokens ?? 0);
  const activeOutput = selectedPoint
    ? selectedPoint.outputTokens
    : (summary?.totalOutputTokens ?? 0);
  const activeCacheWrite = selectedPoint
    ? selectedPoint.cacheCreationTokens
    : (summary?.totalCacheCreationTokens ?? 0);
  const activeCacheRead = selectedPoint
    ? selectedPoint.cacheReadTokens
    : (summary?.totalCacheReadTokens ?? 0);
  const activeTokens = selectedPoint
    ? selectedPoint.tokens
    : (summary?.realTotalTokens ?? 0);
  const activeHitRate = selectedPoint
    ? (selectedPoint.hitRate ?? 0)
    : (summary?.cacheHitRate ?? 0);

  const avgCostPerReq =
    activeCost != null && activeRequests > 0
      ? activeCost / activeRequests
      : null;

  const appChoices = useMemo(() => {
    const items = [...(dimData?.byApp ?? [])]
      .filter((a) => a.summary.totalRequests > 0)
      .sort((a, b) => b.summary.realTotalTokens - a.summary.realTotalTokens);
    if (appFilter && !items.some((a) => a.appType === appFilter)) {
      items.unshift({
        appType: appFilter,
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
      });
    }
    return items;
  }, [dimData?.byApp, appFilter]);

  const providerChoices = useMemo(() => {
    const map = new Map<string, number>();
    for (const p of dimData?.providers ?? []) {
      map.set(p.providerName, (map.get(p.providerName) ?? 0) + p.requestCount);
    }
    if (providerFilter && !map.has(providerFilter)) map.set(providerFilter, 0);
    return Array.from(map.entries())
      .sort((a, b) => b[1] - a[1])
      .slice(0, 8)
      .map(([name]) => name);
  }, [dimData?.providers, providerFilter]);

  const modelChoices = useMemo(() => {
    const sorted = [...(dimData?.models ?? [])].sort(
      (a, b) => b.requestCount - a.requestCount,
    );
    const names = sorted.slice(0, 8).map((m) => m.model);
    if (modelFilter && !names.includes(modelFilter)) names.unshift(modelFilter);
    return names;
  }, [dimData?.models, modelFilter]);

  const activeFilterCount = (providerFilter ? 1 : 0) + (modelFilter ? 1 : 0);
  const hasAnyFilter =
    Boolean(appFilter) || Boolean(providerFilter) || Boolean(modelFilter);

  const selectMetric = (next: UsageCardId) => {
    setMetric(next);
    remember(METRIC_KEY, next);
  };

  const togglePoint = (xKey: string) => {
    setSelectedXKey((prev) => (prev === xKey ? null : xKey));
  };

  const clearAllFilters = () => {
    setAppFilter(undefined);
    setProviderFilter(undefined);
    setModelFilter(undefined);
    setSelectedXKey(null);
  };

  // 下钻细分列表数据（统一映射 App / Provider / Model 三种维度）
  const breakdownItems: BreakdownItem[] = useMemo(() => {
    const targetCard = detailCard ?? metric;
    if (dimension === "app") {
      const rows = (dimData?.byApp ?? []).filter(
        (r) => r.summary.totalRequests > 0 || r.appType === appFilter,
      );
      const val = (r: UsageSummaryByApp) => {
        if (targetCard === "cost")
          return parseFiniteNumber(r.summary.totalCost) ?? 0;
        if (targetCard === "requests") return r.summary.totalRequests;
        if (targetCard === "hitRate") return r.summary.cacheHitRate;
        return r.summary.realTotalTokens;
      };
      const sorted = [...rows].sort((a, b) => val(b) - val(a)).slice(0, 4);
      const maxVal =
        targetCard === "hitRate" ? 1 : Math.max(...sorted.map(val), 0);
      return sorted.map((r) => {
        const v = val(r);
        return {
          key: r.appType,
          label: appDisplayName(r.appType),
          appId: r.appType as AppId,
          valueText:
            targetCard === "cost"
              ? money(r.summary.totalCost, 2)
              : targetCard === "requests"
                ? fmtInt(r.summary.totalRequests, locale)
                : targetCard === "hitRate"
                  ? formatHitRatePercent(r.summary.cacheHitRate)
                  : formatTokensCompact(r.summary.realTotalTokens, locale),
          share: maxVal > 0 ? v / maxVal : 0,
          active: appFilter === r.appType,
          onToggle: () => {
            setSelectedXKey(null);
            setAppFilter((prev) =>
              prev === r.appType ? undefined : r.appType,
            );
          },
        };
      });
    }

    if (dimension === "provider") {
      // 同名供应商在不同 app 下可能多条，按展示名聚合成一行
      const grouped = new Map<
        string,
        {
          name: string;
          requests: number;
          tokens: number;
          cost: number;
          weightedSuccess: number;
        }
      >();
      for (const p of dimData?.providers ?? []) {
        const prev = grouped.get(p.providerName) ?? {
          name: p.providerName,
          requests: 0,
          tokens: 0,
          cost: 0,
          weightedSuccess: 0,
        };
        const c = parseFiniteNumber(p.totalCost) ?? 0;
        prev.requests += p.requestCount;
        prev.tokens += p.totalTokens;
        prev.cost += c;
        prev.weightedSuccess += p.successRate * p.requestCount;
        grouped.set(p.providerName, prev);
      }
      const list = Array.from(grouped.values());
      const val = (p: (typeof list)[number]) => {
        if (targetCard === "cost") return p.cost;
        if (targetCard === "requests") return p.requests;
        if (targetCard === "hitRate") {
          return p.requests > 0 ? p.weightedSuccess / p.requests / 100 : 0;
        }
        return p.tokens;
      };
      const sorted = list.sort((a, b) => val(b) - val(a)).slice(0, 4);
      const maxVal =
        targetCard === "hitRate" ? 1 : Math.max(...sorted.map(val), 0);
      return sorted.map((p) => {
        const v = val(p);
        return {
          key: p.name,
          label: p.name,
          iconName: "",
          valueText:
            targetCard === "cost"
              ? money(p.cost, 2)
              : targetCard === "requests"
                ? fmtInt(p.requests, locale)
                : targetCard === "hitRate"
                  ? `${Math.round(v * 100)}% ${t("usage.tray.successRate")}`
                  : formatTokensCompact(p.tokens, locale),
          share: maxVal > 0 ? v / maxVal : 0,
          active: providerFilter === p.name,
          onToggle: () => {
            setSelectedXKey(null);
            setProviderFilter((prev) => (prev === p.name ? undefined : p.name));
          },
        };
      });
    }

    const models = dimData?.models ?? [];
    const val = (m: ModelStats) => {
      if (targetCard === "cost") return parseFiniteNumber(m.totalCost) ?? 0;
      if (targetCard === "requests") return m.requestCount;
      if (targetCard === "hitRate") return m.successRate / 100;
      return m.totalTokens;
    };
    const sorted = [...models].sort((a, b) => val(b) - val(a)).slice(0, 4);
    const maxVal =
      targetCard === "hitRate" ? 1 : Math.max(...sorted.map(val), 0);
    return sorted.map((m) => {
      const v = val(m);
      return {
        key: m.model,
        label: m.model,
        valueText:
          targetCard === "cost"
            ? money(m.totalCost, 2)
            : targetCard === "requests"
              ? fmtInt(m.requestCount, locale)
              : targetCard === "hitRate"
                ? `${Math.round(m.successRate)}% ${t("usage.tray.successRate")}`
                : formatTokensCompact(m.totalTokens, locale),
        share: maxVal > 0 ? v / maxVal : 0,
        active: modelFilter === m.model,
        onToggle: () => {
          setSelectedXKey(null);
          setModelFilter((prev) => (prev === m.model ? undefined : m.model));
        },
      };
    });
  }, [
    detailCard,
    metric,
    dimension,
    dimData,
    appFilter,
    providerFilter,
    modelFilter,
    money,
    locale,
    t,
  ]);

  // 单卡下钻详情元数据（统一配置表）
  const detailMeta = useMemo(() => {
    if (!detailCard || !summary) return null;
    const scopeText = selectedPoint
      ? `${selectedPoint.tooltipLabel ?? selectedPoint.label}`
      : [
          rangeLabel,
          appFilter ? appDisplayName(appFilter) : null,
          providerFilter ?? null,
          modelFilter ?? null,
        ]
          .filter(Boolean)
          .join(" · ");

    const peakPoint = chartData.reduce<TrayUsagePoint | null>((best, cur) => {
      if (!best) return cur;
      return metricValueOfPoint(cur, detailCard) >
        metricValueOfPoint(best, detailCard)
        ? cur
        : best;
    }, null);

    const peakText =
      peakPoint && metricValueOfPoint(peakPoint, detailCard) > 0
        ? `${t("usage.tray.peak")} ${peakPoint.label} (${formatMetricValue(peakPoint, detailCard)})`
        : null;

    const topItem = breakdownItems[0];
    const insight = [
      peakText,
      topItem ? `${topItem.label} · ${topItem.valueText}` : null,
    ]
      .filter(Boolean)
      .join(" · ");

    const metaMap: Record<
      UsageCardId,
      {
        title: string;
        icon: React.ReactNode;
        heroValue: string;
        subMetrics: DetailSubMetric[];
      }
    > = {
      cost: {
        title: t("usage.totalCost"),
        icon: <Coins className="h-3.5 w-3.5" />,
        heroValue: money(activeCost, 2),
        subMetrics: [
          {
            label: t("usage.tray.avgPerReq"),
            value: avgCostPerReq == null ? "—" : money(avgCostPerReq, 3),
            dotClass: "bg-sky-500",
          },
          {
            label: t("usage.totalRequests"),
            value: fmtInt(activeRequests, locale),
            dotClass: "bg-emerald-500",
          },
        ],
      },
      requests: {
        title: t("usage.totalRequests"),
        icon: <Activity className="h-3.5 w-3.5" />,
        heroValue: fmtInt(activeRequests, locale),
        subMetrics: [
          {
            label: t("usage.tray.successRate"),
            value: `${summary.successRate.toFixed(1)}%`,
            dotClass: "bg-emerald-500",
          },
          {
            label: t("usage.totalCost"),
            value: money(activeCost, 2),
            dotClass: "bg-sky-500",
          },
        ],
      },
      tokens: {
        title: t("usage.realTotal"),
        icon: <Cpu className="h-3.5 w-3.5" />,
        heroValue: formatTokensCompact(activeTokens, locale),
        subMetrics: [
          {
            label: t("usage.inputTokens"),
            value: formatTokensCompact(activeInput, locale),
            dotClass: TOKEN_SEGMENTS[0].dotClass,
          },
          {
            label: t("usage.outputTokens"),
            value: formatTokensCompact(activeOutput, locale),
            dotClass: TOKEN_SEGMENTS[1].dotClass,
          },
          {
            label: t("usage.cacheRead"),
            value: formatTokensCompact(activeCacheRead, locale),
            dotClass: TOKEN_SEGMENTS[3].dotClass,
          },
        ],
      },
      hitRate: {
        title: t("usage.cacheHitRate"),
        icon: <DatabaseZap className="h-3.5 w-3.5" />,
        heroValue: formatHitRatePercent(activeHitRate),
        subMetrics: [
          {
            label: t("usage.cacheRead"),
            value: formatTokensCompact(activeCacheRead, locale),
            dotClass: "bg-emerald-500",
          },
          {
            label: t("usage.cacheWrite"),
            value: formatTokensCompact(activeCacheWrite, locale),
            dotClass: "bg-amber-500",
          },
        ],
      },
    };

    return {
      ...metaMap[detailCard],
      heroSubtitle: scopeText,
      insight: insight || null,
    };
  }, [
    detailCard,
    summary,
    selectedPoint,
    rangeLabel,
    appFilter,
    providerFilter,
    modelFilter,
    chartData,
    breakdownItems,
    activeCost,
    avgCostPerReq,
    activeRequests,
    activeTokens,
    activeInput,
    activeOutput,
    activeCacheRead,
    activeCacheWrite,
    activeHitRate,
    money,
    locale,
    t,
  ]);

  const showFilterBar =
    appChoices.length > 0 ||
    providerChoices.length > 0 ||
    modelChoices.length > 0 ||
    hasAnyFilter;

  return (
    <div className="flex flex-col gap-2 px-3 pb-3">
      {/* 顶层时间范围切换（滑动胶囊过渡） */}
      <Segmented
        label={t("usage.timeRange")}
        value={range}
        onChange={(next) => {
          setRange(next);
          setSelectedXKey(null);
          remember(RANGE_KEY, next);
        }}
        options={RANGES.map((value) => ({
          value,
          label: getUsageRangePresetLabel(value, t),
        }))}
      />

      {/* 应用快捷筛选条 + 供应商/模型筛选浮层 */}
      {showFilterBar && (
        <div
          className="relative flex items-center gap-1.5"
          ref={filterPopoverRef}
        >
          <AppFilterStrip
            label={t("usage.appFilter.label")}
            allLabel={t("usage.tray.allApps")}
            items={appChoices.map((entry) => ({
              appType: entry.appType,
              name: appDisplayName(entry.appType),
            }))}
            selectedApp={appFilter}
            onSelectApp={(nextApp) => {
              setSelectedXKey(null);
              setAppFilter(nextApp);
            }}
          />

          {(providerChoices.length > 0 ||
            modelChoices.length > 0 ||
            activeFilterCount > 0) && (
            <button
              type="button"
              aria-label={t("usage.tray.filterBy")}
              aria-expanded={filterMenuOpen}
              onClick={() => setFilterMenuOpen((open) => !open)}
              className={cn(
                "relative flex h-6 shrink-0 items-center gap-1 rounded-md px-2 text-[11px] font-medium transition-colors",
                filterMenuOpen || activeFilterCount > 0
                  ? "bg-white text-foreground shadow-sm ring-1 ring-sky-500/40 dark:bg-white/[0.16] dark:ring-sky-400/40"
                  : "bg-black/[0.04] text-muted-foreground hover:text-foreground dark:bg-white/[0.06]",
              )}
            >
              <SlidersHorizontal className="h-3 w-3" />
              {activeFilterCount > 0 && (
                <span className="flex h-3.5 min-w-[14px] items-center justify-center rounded-full bg-sky-500 px-1 text-[9.5px] font-semibold text-white">
                  {activeFilterCount}
                </span>
              )}
            </button>
          )}

          {/* 供应商 / 模型级联筛选浮层（参考视频 00:03 的浮层展开过渡） */}
          <AnimatePresence>
            {filterMenuOpen && (
              <motion.div
                initial={{ opacity: 0, scale: 0.96, y: -4 }}
                animate={{ opacity: 1, scale: 1, y: 0 }}
                exit={{ opacity: 0, scale: 0.96, y: -4 }}
                transition={{ duration: 0.14, ease: "easeOut" }}
                className="tp-scroll absolute right-0 top-7 z-30 flex max-h-[240px] w-[230px] flex-col gap-2 rounded-lg border border-black/10 bg-popover p-2 text-popover-foreground shadow-lg dark:border-white/10"
              >
                {providerChoices.length > 0 && (
                  <div className="flex flex-col gap-0.5">
                    <div className="px-1.5 text-[10px] font-medium text-muted-foreground">
                      {t("usage.provider")}
                    </div>
                    {providerChoices.map((name) => {
                      const active = providerFilter === name;
                      return (
                        <button
                          key={name}
                          type="button"
                          onClick={() => {
                            setSelectedXKey(null);
                            setProviderFilter((prev) =>
                              prev === name ? undefined : name,
                            );
                          }}
                          className={cn(
                            "flex w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-left text-[11px] transition-colors",
                            active
                              ? "bg-sky-500/15 font-medium text-foreground"
                              : "hover:bg-black/[0.05] dark:hover:bg-white/[0.08]",
                          )}
                        >
                          <Check
                            className={cn(
                              "h-3 w-3 shrink-0 text-sky-500",
                              !active && "opacity-0",
                            )}
                          />
                          <span className="truncate">{name}</span>
                        </button>
                      );
                    })}
                  </div>
                )}

                {modelChoices.length > 0 && (
                  <div className="flex flex-col gap-0.5 border-t border-black/[0.06] pt-1.5 dark:border-white/[0.08]">
                    <div className="px-1.5 text-[10px] font-medium text-muted-foreground">
                      {t("usage.model")}
                    </div>
                    {modelChoices.map((name) => {
                      const active = modelFilter === name;
                      return (
                        <button
                          key={name}
                          type="button"
                          onClick={() => {
                            setSelectedXKey(null);
                            setModelFilter((prev) =>
                              prev === name ? undefined : name,
                            );
                          }}
                          className={cn(
                            "flex w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-left font-mono text-[10.5px] transition-colors",
                            active
                              ? "bg-sky-500/15 font-medium text-foreground"
                              : "hover:bg-black/[0.05] dark:hover:bg-white/[0.08]",
                          )}
                        >
                          <Check
                            className={cn(
                              "h-3 w-3 shrink-0 text-sky-500",
                              !active && "opacity-0",
                            )}
                          />
                          <span className="truncate">{name}</span>
                        </button>
                      );
                    })}
                  </div>
                )}

                {hasAnyFilter && (
                  <button
                    type="button"
                    onClick={() => {
                      clearAllFilters();
                      setFilterMenuOpen(false);
                    }}
                    className="rounded-md bg-black/[0.05] px-2 py-1 text-center text-[11px] font-medium text-muted-foreground transition-colors hover:text-foreground dark:bg-white/[0.08]"
                  >
                    {t("usage.tray.clearFilters")}
                  </button>
                )}
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      )}

      {/* 已选条件标签条（供应商 / 模型 / 图表联动时段），一键取消 */}
      {(providerFilter || modelFilter || selectedPoint) && (
        <div className="flex flex-wrap items-center gap-1">
          {providerFilter && (
            <button
              type="button"
              onClick={() => setProviderFilter(undefined)}
              className="inline-flex items-center gap-1 rounded-md bg-sky-500/15 px-1.5 py-0.5 text-[10.5px] font-medium text-sky-700 dark:text-sky-300"
            >
              <span className="max-w-[110px] truncate">{providerFilter}</span>
              <X className="h-2.5 w-2.5 shrink-0" />
            </button>
          )}
          {modelFilter && (
            <button
              type="button"
              onClick={() => setModelFilter(undefined)}
              className="inline-flex items-center gap-1 rounded-md bg-sky-500/15 px-1.5 py-0.5 font-mono text-[10px] font-medium text-sky-700 dark:text-sky-300"
            >
              <span className="max-w-[120px] truncate">{modelFilter}</span>
              <X className="h-2.5 w-2.5 shrink-0" />
            </button>
          )}
          {selectedPoint && (
            <button
              type="button"
              onClick={() => setSelectedXKey(null)}
              className="inline-flex items-center gap-1 rounded-md bg-black/[0.07] px-1.5 py-0.5 text-[10.5px] font-medium text-foreground dark:bg-white/[0.14]"
            >
              <span>
                {t("usage.tray.selectedBucket")}: {selectedPoint.label}
              </span>
              <X className="h-2.5 w-2.5 shrink-0" />
            </button>
          )}
        </div>
      )}

      {!summary && isLoading ? (
        <div className="flex h-40 items-center justify-center text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" />
        </div>
      ) : !summary || summary.totalRequests === 0 ? (
        <div className="flex h-24 flex-col items-center justify-center gap-1.5 text-xs text-muted-foreground">
          <span>{t("usage.noData")}</span>
          {hasAnyFilter && (
            <button
              type="button"
              onClick={clearAllFilters}
              className="rounded-md bg-black/[0.06] px-2 py-0.5 text-[11px] text-foreground transition-colors hover:bg-black/[0.1] dark:bg-white/[0.1] dark:hover:bg-white/[0.15]"
            >
              {t("usage.tray.clearFilters")}
            </button>
          )}
        </div>
      ) : (
        <LayoutGroup id="tray-usage-bento">
          {detailCard && detailMeta ? (
            <UsageCardDetailView
              cardId={detailCard}
              title={detailMeta.title}
              icon={detailMeta.icon}
              heroValue={detailMeta.heroValue}
              heroSubtitle={detailMeta.heroSubtitle}
              subMetrics={detailMeta.subMetrics}
              chart={
                <UsageTrendGraphic
                  chartType={chartType}
                  chartData={chartData}
                  metric={detailCard}
                  selectedXKey={selectedXKey}
                  onSelectPoint={togglePoint}
                  currencySymbol={currencySymbol}
                  tickFormatter={tickFormatter}
                  formatValue={formatMetricValue}
                  hitRateFloor={hitRateFloor}
                  heightClass="h-[108px]"
                />
              }
              dimension={dimension}
              onDimensionChange={setDimension}
              dimensionOptions={[
                { value: "app", label: t("usage.tray.byApp") },
                { value: "provider", label: t("usage.tray.byProvider") },
                { value: "model", label: t("usage.tray.byModel") },
              ]}
              dimensionLabel={t("usage.tray.viewDetails")}
              breakdownItems={breakdownItems}
              noBreakdownText={t("usage.tray.noBreakdown")}
              insight={detailMeta.insight}
              backLabel={t("usage.tray.backToOverview")}
              onBack={() => setDetailCard(null)}
            />
          ) : (
            <div className="flex flex-col gap-2">
              {/* 上半：2×2 指标卡（数值 + 统计图形，点击联动指标，点右上角或再点一次下钻） */}
              <div className="grid grid-cols-2 gap-1.5">
                <MetricBentoCard
                  id="cost"
                  label={t("usage.totalCost")}
                  value={money(activeCost, 2)}
                  detail={
                    avgCostPerReq == null
                      ? undefined
                      : `${t("usage.tray.avgPerReq")} ${money(avgCostPerReq, 3)}`
                  }
                  graphic={
                    <MiniCostBars
                      points={chartData}
                      selectedXKey={selectedXKey}
                    />
                  }
                  selected={metric === "cost"}
                  expandLabel={t("usage.tray.viewDetails")}
                  onSelectMetric={() => selectMetric("cost")}
                  onDrillDown={() => setDetailCard("cost")}
                />
                <MetricBentoCard
                  id="requests"
                  label={t("usage.totalRequests")}
                  value={fmtInt(activeRequests, locale)}
                  detail={`${t("usage.tray.successRate")} ${summary.successRate.toFixed(1)}%`}
                  graphic={
                    <MiniRequestsSpark
                      points={chartData}
                      selectedXKey={selectedXKey}
                    />
                  }
                  selected={metric === "requests"}
                  expandLabel={t("usage.tray.viewDetails")}
                  onSelectMetric={() => selectMetric("requests")}
                  onDrillDown={() => setDetailCard("requests")}
                />
                <MetricBentoCard
                  id="tokens"
                  label={t("usage.realTotal")}
                  value={formatTokensCompact(activeTokens, locale)}
                  footerGraphic={
                    <StackedTokenBar
                      input={activeInput}
                      output={activeOutput}
                      cacheWrite={activeCacheWrite}
                      cacheRead={activeCacheRead}
                    />
                  }
                  detail={`${t("usage.inputTokens")} ${formatTokensCompact(activeInput, locale)} · ${t("usage.outputTokens")} ${formatTokensCompact(activeOutput, locale)}`}
                  selected={metric === "tokens"}
                  expandLabel={t("usage.tray.viewDetails")}
                  onSelectMetric={() => selectMetric("tokens")}
                  onDrillDown={() => setDetailCard("tokens")}
                />
                <MetricBentoCard
                  id="hitRate"
                  label={t("usage.cacheHitRate")}
                  value={formatHitRatePercent(activeHitRate)}
                  graphic={<HitRateRing rate={activeHitRate} />}
                  detail={`${t("usage.cacheWrite")} ${formatTokensCompact(activeCacheWrite, locale)} · ${t("usage.cacheRead")} ${formatTokensCompact(activeCacheRead, locale)}`}
                  selected={metric === "hitRate"}
                  expandLabel={t("usage.tray.viewDetails")}
                  onSelectMetric={() => selectMetric("hitRate")}
                  onDrillDown={() => setDetailCard("hitRate")}
                />
              </div>

              {/* 下半：交互式趋势图（点击柱子/点联动上方卡片数值，切换指标与卡片同步） */}
              <div className="rounded-lg bg-black/[0.04] px-2 pb-1 pt-2 dark:bg-white/[0.06]">
                <div className="mb-1 flex items-center gap-1.5 px-1">
                  <span className="truncate text-[11px] font-medium text-muted-foreground">
                    {t("usage.trend.title", { range: rangeLabel })}
                  </span>
                  <div className="ms-auto">
                    <Segmented
                      size="sm"
                      label={t("usage.trend.metricLabel")}
                      value={metric}
                      onChange={selectMetric}
                      options={[
                        { value: "tokens", label: t("usage.trend.tokens") },
                        {
                          value: "requests",
                          label: t("usage.trend.requests"),
                        },
                        { value: "cost", label: t("usage.trend.cost") },
                        {
                          value: "hitRate",
                          label: t("usage.tray.hitRateShort"),
                        },
                      ]}
                    />
                  </div>
                </div>
                <UsageTrendGraphic
                  chartType={chartType}
                  chartData={chartData}
                  metric={metric}
                  selectedXKey={selectedXKey}
                  onSelectPoint={togglePoint}
                  currencySymbol={currencySymbol}
                  tickFormatter={tickFormatter}
                  formatValue={formatMetricValue}
                  hitRateFloor={hitRateFloor}
                  heightClass="h-[124px]"
                />
              </div>
            </div>
          )}
        </LayoutGroup>
      )}
    </div>
  );
}
