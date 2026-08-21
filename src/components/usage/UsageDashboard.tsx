import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "@/lib/toast";
import {
  Check,
  ChartColumn,
  ChevronDown,
  Database,
  Loader2,
  RefreshCw,
} from "lucide-react";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { HelpTip } from "@/components/ui/help-tip";
import { PageTabs } from "@/components/ui/page-tabs";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import {
  SegmentThumb,
  useSlidingIndicator,
} from "@/components/ui/sliding-indicator";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  KNOWN_APP_TYPES,
  type AppTypeFilter,
  type UsageRangeSelection,
} from "@/types/usage";
import {
  usageKeys,
  useModelStats,
  useProviderStats,
  useSessionUsageLastSync,
} from "@/lib/query/usage";
import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
import { usageApi } from "@/lib/api/usage";
import { getUsageRangePresetLabel, resolveUsageRange } from "@/lib/usageRange";
import { cn } from "@/lib/utils";
import { UsageHero } from "./UsageHero";
import { UsageTrendChart } from "./UsageTrendChart";
import { RequestLogTable } from "./RequestLogTable";
import { ProviderStatsTable } from "./ProviderStatsTable";
import { ModelStatsTable } from "./ModelStatsTable";
import { PricingConfigPanel } from "./PricingConfigPanel";
import { RequestDetailPanel } from "./RequestDetailPanel";
import { UsageDataSourcesSheet } from "./UsageDataSourcesSheet";
import { UsageDateRangePicker } from "./UsageDateRangePicker";
import { UsageHeatmap } from "./UsageHeatmap";
import { fmtInt, formatRelativeTime, getLocaleFromLanguage } from "./format";
import { getUsageProviderLabel, usageProviderTitle } from "./providerLabel";

const DEFAULT_REFRESH_INTERVAL_MS = 30000;
const REFRESH_INTERVAL_OPTIONS_MS = [0, 5000, 10000, 30000, 60000] as const;
type RefreshIntervalOption = (typeof REFRESH_INTERVAL_OPTIONS_MS)[number];

const isRefreshIntervalOption = (
  value: number | undefined,
): value is RefreshIntervalOption =>
  REFRESH_INTERVAL_OPTIONS_MS.includes(value as RefreshIntervalOption);

const normalizeRefreshInterval = (value: number | undefined) =>
  isRefreshIntervalOption(value) ? value : DEFAULT_REFRESH_INTERVAL_MS;

const STATUS_CODE_OPTIONS = [200, 400, 401, 429, 500] as const;

type UsageTab = "logs" | "providers" | "models" | "pricing";
const TABS: UsageTab[] = ["logs", "providers", "models", "pricing"];

/**
 * 手动「立即同步」的时间，离开页面再回来也还在。后端也会记下最近一次扫描（后台定时和
 * 手动都算）完成的时间，页头取两者中较新的；这里留着是为了点完同步立刻显示「刚刚」。
 */
let lastManualSessionSyncAt: number | null = null;

/** 测试用：重置模块级的上次同步时间。 */
export function resetUsageSyncClockForTests() {
  lastManualSessionSyncAt = null;
}

/** 容器宽度（ResizeObserver）；测量不到（jsdom）时返回 0。 */
function useContainerWidth<T extends HTMLElement>() {
  const ref = useRef<T | null>(null);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const measure = () => setWidth(element.getBoundingClientRect().width);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return [ref, width] as const;
}

/** 每 30 秒重渲染一次，让「N 分钟前同步」跟着走。 */
function useMinuteTicker() {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, []);
  return now;
}

const menuContentClass =
  "min-w-[200px] max-w-[320px] rounded-panel border-border bg-surface p-1 shadow-v7-md";
const menuItemClass = "h-[30px] gap-2 rounded-control px-2 text-body";

function MenuCheck({ checked }: { checked: boolean }) {
  return (
    <Check
      aria-hidden="true"
      strokeWidth={1.75}
      className={cn("h-3.5 w-3.5 shrink-0", !checked && "invisible")}
    />
  );
}

function MenuMeta({ children }: { children: React.ReactNode }) {
  return (
    <span className="ms-auto ps-3 text-caption tabular-nums text-fg-3">
      {children}
    </span>
  );
}

export interface UsageDefaultFilter {
  appType: AppTypeFilter;
  providerName?: string;
  revision: number;
}

interface UsageDashboardProps {
  defaultFilter?: UsageDefaultFilter;
  refreshIntervalMs?: number;
  onRefreshIntervalChange?: (next: number) => Promise<boolean> | boolean | void;
  sessionAutoSyncEnabled?: boolean;
  onSessionAutoSyncEnabledChange?: (
    next: boolean,
  ) => Promise<boolean> | boolean | void;
  /** 从应用页「查看此应用的用量」进入时带上的应用筛选 */
  initialAppType?: AppTypeFilter;
  /** 「数据来源」里的「修改记录请求用量」：打开设置 → 本地路由 */
  onOpenRoutingSettings?: () => void;
}

/**
 * 用量统计全局页（v7 S6）：页头 → 筛选行 → 指标 → 趋势图 → 子页签（请求日志 / 供应商 / 模型 / 定价）。
 */
export function UsageDashboard({
  refreshIntervalMs: savedRefreshIntervalMs,
  onRefreshIntervalChange,
  sessionAutoSyncEnabled = true,
  onSessionAutoSyncEnabledChange,
  initialAppType = "all",
  defaultFilter,
  onOpenRoutingSettings,
}: UsageDashboardProps = {}) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const [range, setRange] = useState<UsageRangeSelection>({
    preset: "today",
  });
  const [appType, setAppType] = useState<AppTypeFilter>(initialAppType);
  const [providerName, setProviderName] = useState<string | undefined>(
    undefined,
  );
  const [model, setModel] = useState<string | undefined>(undefined);
  const [statusCode, setStatusCode] = useState<number | undefined>(undefined);
  const [tab, setTab] = useState<UsageTab>("logs");
  // 没有用量时也能进定价页：新装的人往往先配好 models.dev 同步再开始用
  const [pricingWhileEmpty, setPricingWhileEmpty] = useState(false);
  const [refreshIntervalMs, setRefreshIntervalMs] = useState(() =>
    normalizeRefreshInterval(savedRefreshIntervalMs),
  );
  const [detailRequestId, setDetailRequestId] = useState<string | null>(null);
  const [sourcesOpen, setSourcesOpen] = useState(false);
  const [showRebuildConfirm, setShowRebuildConfirm] = useState(false);
  const [rebuildingCodex, setRebuildingCodex] = useState(false);
  const [syncingSession, setSyncingSession] = useState(false);
  const [lastManualSyncAt, setLastManualSyncAt] = useState(
    lastManualSessionSyncAt,
  );
  const { data: lastScanAt } = useSessionUsageLastSync();
  const lastSyncAt = Math.max(lastManualSyncAt ?? 0, lastScanAt ?? 0) || null;
  const [containerRef, measuredWidth] = useContainerWidth<HTMLDivElement>();
  const now = useMinuteTicker();

  // 测量不到（测试环境）按宽窗口排
  const width = measuredWidth || 1000;
  const compact = width < 720;
  const showSyncText = width >= 780;

  useEffect(() => {
    setRefreshIntervalMs(normalizeRefreshInterval(savedRefreshIntervalMs));
  }, [savedRefreshIntervalMs]);

  useEffect(() => {
    setAppType(defaultFilter?.appType ?? initialAppType);
    setProviderName(defaultFilter?.providerName);
    setModel(undefined);
  }, [initialAppType, defaultFilter]);

  // 切应用时清掉下游筛选，避免留下一个在新范围内查无数据的"幽灵"组合；
  // 切供应商同理清掉模型（模型选项随供应商级联）。
  const changeAppType = (next: AppTypeFilter) => {
    setAppType(next);
    if (next !== appType) {
      setProviderName(undefined);
      setModel(undefined);
    }
  };
  const changeProviderName = (next: string | undefined) => {
    setProviderName(next);
    if (next !== providerName) {
      setModel(undefined);
    }
  };

  // 后端写入新日志时 emit `usage-log-recorded`，立刻 invalidate 所有 usage 查询
  useUsageEventBridge();

  const changeRefreshInterval = async (next: number) => {
    const normalized = normalizeRefreshInterval(next);
    const previous = refreshIntervalMs;
    setRefreshIntervalMs(normalized);
    queryClient.invalidateQueries({ queryKey: usageKeys.all });
    try {
      const saved = await onRefreshIntervalChange?.(normalized);
      if (saved === false) {
        setRefreshIntervalMs(previous);
      }
    } catch (error) {
      console.error(
        "[UsageDashboard] Failed to persist refresh interval",
        error,
      );
      setRefreshIntervalMs(previous);
    }
  };

  const rebuildCodexUsage = async () => {
    setShowRebuildConfirm(false);
    setRebuildingCodex(true);
    try {
      const result = await usageApi.rebuildCodexUsage();
      await queryClient.invalidateQueries({ queryKey: usageKeys.all });
      const message = t("usage.rebuildCodex.completed", {
        imported: result.imported,
        errors: result.errors.length,
        suspected: result.suspectedDuplicates,
        deferred: result.deferredFiles,
      });
      if (result.errors.length > 0 || result.deferredFiles > 0) {
        toast.warning(message);
      } else {
        toast.success(message);
      }
    } catch (error) {
      toast.error(t("usage.rebuildCodex.failed", { error: String(error) }));
    } finally {
      setRebuildingCodex(false);
    }
  };

  const runManualSessionSync = async () => {
    setSyncingSession(true);
    try {
      const result = await usageApi.syncSessionUsage();
      await queryClient.invalidateQueries({ queryKey: usageKeys.all });
      lastManualSessionSyncAt = Date.now();
      setLastManualSyncAt(lastManualSessionSyncAt);
      const message = t("usage.sessionSync.syncCompleted", {
        imported: result.imported,
        files: result.filesScanned,
        errors: result.errors.length,
      });
      if (result.errors.length > 0) {
        toast.warning(message);
      } else {
        toast.success(message);
      }
    } catch (error) {
      toast.error(t("usage.sessionSync.syncFailed", { error: String(error) }));
    } finally {
      setSyncingSession(false);
    }
  };

  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);
  const resolvedRange = useMemo(() => resolveUsageRange(range), [range]);
  const rangeLabel = useMemo(() => {
    if (range.preset !== "custom") {
      return getUsageRangePresetLabel(range.preset, t);
    }
    const startStr = new Date(resolvedRange.startDate * 1000).toLocaleString(
      locale,
    );
    if (range.liveEndTime) {
      return `${startStr} → ${t("usage.liveEndTimeNow", "现在")}`;
    }
    const endStr = new Date(resolvedRange.endDate * 1000).toLocaleString(
      locale,
    );
    return `${startStr} - ${endStr}`;
  }, [locale, range, resolvedRange.endDate, resolvedRange.startDate, t]);

  // 筛选下拉的选项池：供应商列表只跟应用/时间范围走（不受自身选中值影响），
  // 模型列表随所选供应商级联。refetchInterval 跟随面板的刷新设置——未筛选时
  // 这两个查询与统计表共享 query key，落下的话会按默认 30s 拖着同 key 查询轮询。
  const refetch = {
    refetchInterval:
      refreshIntervalMs > 0 ? refreshIntervalMs : (false as const),
  };
  const { data: providerOptionsData } = useProviderStats(
    range,
    { appType },
    refetch,
  );
  const { data: modelOptionsData } = useModelStats(
    range,
    { appType, providerName },
    refetch,
  );
  // 有没有任何用量（不分时间范围）：一条都没有时显示空状态
  const { data: allTimeSummary } = useQuery({
    queryKey: [...usageKeys.all, "all-time-summary"],
    queryFn: () => usageApi.getUsageSummary(),
    refetchInterval: refetch.refetchInterval,
  });
  const isEmpty = allTimeSummary != null && allTimeSummary.totalRequests === 0;

  const providerOptions = useMemo(() => {
    const counts = new Map<string, number>();
    for (const stat of providerOptionsData ?? []) {
      counts.set(
        stat.providerName,
        (counts.get(stat.providerName) ?? 0) + stat.requestCount,
      );
    }
    // 数据刷新后选中项可能掉出列表（如改了时间范围）；补回去保证用户看得见、能清除
    if (providerName && !counts.has(providerName)) counts.set(providerName, 0);
    return Array.from(counts, ([name, count]) => ({ name, count })).sort(
      (a, b) => b.count - a.count,
    );
  }, [providerOptionsData, providerName]);

  const modelOptions = useMemo(() => {
    const counts = new Map<string, number>();
    for (const stat of modelOptionsData ?? []) {
      counts.set(stat.model, (counts.get(stat.model) ?? 0) + stat.requestCount);
    }
    if (model && !counts.has(model)) counts.set(model, 0);
    return Array.from(counts, ([name, count]) => ({ name, count })).sort(
      (a, b) => b.count - a.count,
    );
  }, [modelOptionsData, model]);

  const providerTotal = providerOptions.reduce((sum, p) => sum + p.count, 0);
  const modelTotal = modelOptions.reduce((sum, m) => sum + m.count, 0);

  // ── 页头 ─────────────────────────────────────────────────────────────
  const syncedLabel = useMemo(() => {
    if (lastSyncAt == null) return undefined;
    const minutes = Math.max(0, Math.floor((now - lastSyncAt) / 60_000));
    return minutes < 1
      ? t("usage.syncStatus.justNow")
      : t("usage.syncStatus.minutesAgo", { count: minutes });
  }, [lastSyncAt, now, t]);
  const syncText = !sessionAutoSyncEnabled
    ? t("usage.syncStatus.off")
    : (syncedLabel ?? t("usage.syncStatus.auto"));
  const syncTip = sessionAutoSyncEnabled
    ? t("usage.syncStatus.tipAuto")
    : t("usage.syncStatus.tipOff");
  const drawerSyncedLabel =
    lastSyncAt == null
      ? undefined
      : t("usage.sources.lastSynced", {
          time: formatRelativeTime(lastSyncAt, t, now),
        });

  const refreshLabel =
    refreshIntervalMs > 0
      ? t("usage.refreshMenu.label", { seconds: refreshIntervalMs / 1000 })
      : t("usage.refreshMenu.off");

  const headerActions = (
    <>
      {showSyncText && (
        <span
          role="status"
          title={syncTip}
          className="min-w-0 truncate whitespace-nowrap text-caption tabular-nums text-fg-3"
        >
          {syncText}
        </span>
      )}
      <HoverTip content={showSyncText ? syncTip : `${syncText} · ${syncTip}`}>
        <Button
          type="button"
          variant="neutral"
          size="regular"
          className="shrink-0 gap-1.5 ps-2.5"
          disabled={syncingSession}
          onClick={() => void runManualSessionSync()}
        >
          {syncingSession ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <RefreshCw className="h-3.5 w-3.5" />
          )}
          {t("usage.sessionSync.syncNow")}
        </Button>
      </HoverTip>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            type="button"
            variant="quiet"
            size="regular"
            className="shrink-0 gap-1 pe-2 ps-2.5 text-fg-2"
            aria-label={`${t("usage.refreshInterval")}: ${refreshLabel}`}
          >
            {refreshLabel}
            <ChevronDown className="h-3.5 w-3.5" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          className={cn(menuContentClass, "min-w-[150px]")}
        >
          {REFRESH_INTERVAL_OPTIONS_MS.map((ms) => (
            <DropdownMenuItem
              key={ms}
              className={menuItemClass}
              onSelect={() => void changeRefreshInterval(ms)}
            >
              <MenuCheck checked={refreshIntervalMs === ms} />
              {ms > 0
                ? t("usage.refreshMenu.seconds", { seconds: ms / 1000 })
                : t("usage.refreshOff")}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
      <Button
        type="button"
        variant="quiet"
        size="regular"
        className="shrink-0 gap-1.5 ps-2.5 text-fg-2"
        aria-haspopup="dialog"
        onClick={() => setSourcesOpen(true)}
      >
        <Database className="h-3.5 w-3.5" />
        {t("usage.dataSources")}
      </Button>
    </>
  );

  // ── 筛选行 ───────────────────────────────────────────────────────────
  // 选中块是单独的滑块（SegmentThumb），切换应用时滑过去
  const appFilterIndicator = useSlidingIndicator<HTMLDivElement>(
    '[aria-pressed="true"]',
    appType,
  );
  const chipClass = (pressed: boolean) =>
    cn(
      "relative inline-flex h-[26px] shrink-0 items-center gap-1.5 whitespace-nowrap rounded-[5px] px-2 text-body transition-colors duration-150",
      pressed
        ? "font-semibold text-fg-1"
        : "font-medium text-fg-2 hover:text-fg-1",
    );

  const filterRow = (
    <div className="flex min-h-12 shrink-0 flex-wrap items-center gap-1.5 px-6 py-2">
      <div
        ref={appFilterIndicator.ref}
        role="group"
        aria-label={t("usage.appFilter.label")}
        className="relative flex h-8 shrink-0 items-center gap-0.5 rounded-[8px] bg-subtle p-[3px]"
      >
        <SegmentThumb
          rect={appFilterIndicator.rect}
          animate={appFilterIndicator.animate}
          className="rounded-[5px]"
        />
        <button
          type="button"
          aria-pressed={appType === "all"}
          className={chipClass(appType === "all")}
          onClick={() => changeAppType("all")}
        >
          {t("usage.appFilter.all")}
        </button>
        {/* 应用只露图标，名字放悬停提示和 aria-label */}
        {KNOWN_APP_TYPES.map((app) => (
          <HoverTip key={app} content={APP_DISPLAY_NAME[app]}>
            <button
              type="button"
              aria-pressed={appType === app}
              aria-label={APP_DISPLAY_NAME[app]}
              className={cn(
                chipClass(appType === app),
                "w-[30px] justify-center px-0",
              )}
              onClick={() => changeAppType(app)}
            >
              <AppGlyph
                app={app}
                size={16}
                badgeClassName={appType === app ? "bg-surface" : "bg-subtle"}
              />
            </button>
          </HoverTip>
        ))}
      </div>
      <div className="min-w-2 flex-1" />
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            type="button"
            variant="neutral"
            size="regular"
            className="min-w-[78px] max-w-[156px] shrink gap-1 pe-2 ps-3"
            title={
              providerName
                ? usageProviderTitle(getUsageProviderLabel(providerName, t))
                : t("usage.providerFilter.title")
            }
          >
            <span className="min-w-0 truncate">
              {providerName
                ? getUsageProviderLabel(providerName, t).label
                : t("usage.providerFilter.label")}
            </span>
            <ChevronDown className="h-3.5 w-3.5 shrink-0 text-fg-2" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          aria-label={t("usage.providerFilter.title")}
          className={cn(menuContentClass, "max-h-[360px] overflow-y-auto")}
        >
          <DropdownMenuItem
            className={menuItemClass}
            onSelect={() => changeProviderName(undefined)}
          >
            <MenuCheck checked={providerName == null} />
            {t("usage.providerFilter.all")}
            <MenuMeta>{fmtInt(providerTotal, locale)}</MenuMeta>
          </DropdownMenuItem>
          {providerOptions.map((option) => {
            const label = getUsageProviderLabel(option.name, t);
            return (
              <DropdownMenuItem
                key={option.name}
                className={menuItemClass}
                title={usageProviderTitle(label)}
                onSelect={() => changeProviderName(option.name)}
              >
                <MenuCheck checked={providerName === option.name} />
                <span className="min-w-0 truncate">{label.label}</span>
                <MenuMeta>{fmtInt(option.count, locale)}</MenuMeta>
              </DropdownMenuItem>
            );
          })}
        </DropdownMenuContent>
      </DropdownMenu>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            type="button"
            variant="neutral"
            size="regular"
            className="min-w-[66px] max-w-[156px] shrink gap-1 pe-2 ps-3"
            title={model ?? t("usage.modelFilter.title")}
          >
            <span className="min-w-0 truncate">
              {model ?? t("usage.modelFilter.label")}
            </span>
            <ChevronDown className="h-3.5 w-3.5 shrink-0 text-fg-2" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          aria-label={t("usage.modelFilter.title")}
          className={cn(menuContentClass, "max-h-[360px] overflow-y-auto")}
        >
          <DropdownMenuItem
            className={menuItemClass}
            onSelect={() => setModel(undefined)}
          >
            <MenuCheck checked={model == null} />
            {t("usage.allModels")}
            <MenuMeta>{fmtInt(modelTotal, locale)}</MenuMeta>
          </DropdownMenuItem>
          {modelOptions.map((option) => (
            <DropdownMenuItem
              key={option.name}
              className={menuItemClass}
              title={option.name}
              onSelect={() => setModel(option.name)}
            >
              <MenuCheck checked={model === option.name} />
              <span className="min-w-0 truncate font-mono text-caption">
                {option.name}
              </span>
              <MenuMeta>{fmtInt(option.count, locale)}</MenuMeta>
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
      <UsageDateRangePicker
        selection={range}
        triggerLabel={rangeLabel}
        onApply={(nextRange) => setRange(nextRange)}
      />
    </div>
  );

  // ── 子页签 ───────────────────────────────────────────────────────────
  const tabLabel: Record<UsageTab, string> = {
    logs: t("usage.requestLogs"),
    providers: t("usage.tabs.providers"),
    models: t("usage.tabs.models"),
    pricing: t("usage.tabs.pricing"),
  };
  const statusLabel =
    statusCode == null
      ? t("usage.statusFilter.all")
      : `${t("usage.statusCode")} ${statusCode}`;

  const tabTrailing =
    tab === "logs" ? (
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            type="button"
            variant="quiet"
            size="compact"
            className="gap-1 pe-1.5 ps-2 text-caption text-fg-2"
            aria-label={`${t("usage.statusCode")}: ${statusLabel}`}
          >
            {statusLabel}
            <ChevronDown className="h-3.5 w-3.5" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          className={cn(menuContentClass, "min-w-[160px]")}
        >
          <DropdownMenuItem
            className={menuItemClass}
            onSelect={() => setStatusCode(undefined)}
          >
            <MenuCheck checked={statusCode == null} />
            {t("usage.statusFilter.all")}
          </DropdownMenuItem>
          {STATUS_CODE_OPTIONS.map((code) => (
            <DropdownMenuItem
              key={code}
              className={menuItemClass}
              onSelect={() => setStatusCode(code)}
            >
              <MenuCheck checked={statusCode === code} />
              <span className="tabular-nums">{code}</span>
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    ) : tab === "providers" || tab === "models" ? (
      <span className="text-caption text-fg-3">
        {t("usage.sortedByRequests")}
      </span>
    ) : null;

  const scopedAppType = appType === "all" ? undefined : appType;

  const tabsSection = (
    <section aria-label={t("usage.tabs.label")} className="flex flex-col">
      <PageTabs<UsageTab>
        aria-label={t("usage.tabs.label")}
        items={TABS.map((id) => ({ value: id, label: tabLabel[id] }))}
        value={tab}
        onValueChange={setTab}
        idPrefix="usage-tab"
        controls="usage-tabpanel"
        trailing={tabTrailing}
      />

      <div
        role="tabpanel"
        id="usage-tabpanel"
        aria-labelledby={`usage-tab-${tab}`}
      >
        {tab === "logs" && (
          <RequestLogTable
            range={range}
            appType={appType}
            providerName={providerName}
            model={model}
            statusCode={statusCode}
            refreshIntervalMs={refreshIntervalMs}
            onOpenDetail={setDetailRequestId}
          />
        )}
        {tab === "providers" && (
          <ProviderStatsTable
            range={range}
            appType={appType}
            providerName={providerName}
            model={model}
            refreshIntervalMs={refreshIntervalMs}
          />
        )}
        {tab === "models" && (
          <ModelStatsTable
            range={range}
            appType={appType}
            providerName={providerName}
            model={model}
            refreshIntervalMs={refreshIntervalMs}
          />
        )}
        {tab === "pricing" && <PricingConfigPanel />}
      </div>
    </section>
  );

  const body =
    isEmpty && pricingWhileEmpty ? (
      // 空库里只有定价可配：不画全是 0 的概览和趋势，直接给页签
      <div
        id="main-content"
        className="flex min-h-0 flex-1 flex-col gap-3.5 overflow-y-auto scroll-stable px-6 pb-6 pt-1"
      >
        {tabsSection}
      </div>
    ) : isEmpty ? (
      <div className="flex min-h-0 flex-1 flex-col items-center overflow-y-auto scroll-stable px-6 pb-6">
        <div className="mt-16 flex max-w-[440px] flex-col items-center gap-3 text-center">
          <span
            aria-hidden="true"
            className="flex h-11 w-11 items-center justify-center rounded-full bg-subtle text-fg-2"
          >
            <ChartColumn className="h-5 w-5" strokeWidth={1.5} />
          </span>
          <div className="flex flex-col gap-1">
            <p className="m-0 text-section text-fg-1">
              {t("usage.empty.title")}
            </p>
            <p className="m-0 text-body text-fg-2">{t("usage.empty.body")}</p>
          </div>
          <Button
            type="button"
            variant="neutral"
            size="regular"
            disabled={syncingSession}
            onClick={() => void runManualSessionSync()}
          >
            {syncingSession && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {t("usage.sessionSync.syncNow")}
          </Button>
          <Button
            type="button"
            variant="quiet"
            size="regular"
            onClick={() => {
              setTab("pricing");
              setPricingWhileEmpty(true);
            }}
          >
            {t("usage.empty.configurePricing")}
          </Button>
        </div>
      </div>
    ) : (
      <div
        id="main-content"
        className="flex min-h-0 flex-1 flex-col gap-3.5 overflow-y-auto scroll-stable px-6 pb-6 pt-1"
      >
        <UsageHero
          range={range}
          appType={scopedAppType}
          providerName={providerName}
          model={model}
          refreshIntervalMs={refreshIntervalMs}
          compact={compact}
        />

        {/* 「全部」看长期分布用热力图；24 小时到 30 天这类短范围用柱状图 */}
        {range.preset === "all" ? (
          <UsageHeatmap
            appType={appType}
            providerName={providerName}
            model={model}
            refreshIntervalMs={refreshIntervalMs}
          />
        ) : (
          <UsageTrendChart
            range={range}
            rangeLabel={rangeLabel}
            appType={appType}
            providerName={providerName}
            model={model}
            refreshIntervalMs={refreshIntervalMs}
          />
        )}

        {tabsSection}
      </div>
    );

  return (
    <div ref={containerRef} className="flex min-h-0 min-w-0 flex-1 flex-col">
      <AppPageHeader
        icon={<ChartColumn className="h-5 w-5" strokeWidth={1.5} />}
        title={t("nav.usage")}
        titleExtra={
          <HelpTip title={t("nav.usage")}>{t("usage.subtitle")}</HelpTip>
        }
        actions={headerActions}
      />
      {filterRow}
      {body}

      <RequestDetailPanel
        requestId={detailRequestId}
        onClose={() => setDetailRequestId(null)}
      />
      <UsageDataSourcesSheet
        open={sourcesOpen}
        onOpenChange={setSourcesOpen}
        sessionAutoSyncEnabled={sessionAutoSyncEnabled}
        onSessionAutoSyncEnabledChange={(value) =>
          void onSessionAutoSyncEnabledChange?.(value)
        }
        syncedLabel={drawerSyncedLabel}
        syncing={syncingSession}
        onSyncNow={() => void runManualSessionSync()}
        onOpenRoutingSettings={onOpenRoutingSettings}
        rebuildingCodex={rebuildingCodex}
        onRebuildCodex={() => setShowRebuildConfirm(true)}
      />
      <ConfirmDialog
        isOpen={showRebuildConfirm}
        title={t("usage.rebuildCodex.confirmTitle")}
        message={t("usage.rebuildCodex.confirmMessage")}
        confirmText={t("usage.rebuildCodex.confirmAction")}
        // 重建前会自动备份数据库，可从备份恢复：不算不可撤销，不用红色确认键
        variant="info"
        zIndex="top"
        onConfirm={() => void rebuildCodexUsage()}
        onCancel={() => setShowRebuildConfirm(false)}
      />
    </div>
  );
}
