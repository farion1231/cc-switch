import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import { motion } from "framer-motion";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  Cell,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { ChevronLeft, ChevronRight, Maximize2, Minimize2 } from "lucide-react";
import { AppGlyph } from "@/components/shell/AppGlyph";
import { ProviderIcon } from "@/components/ProviderIcon";
import {
  formatUsageTrendTokenTickLabel,
  type UsageTrendChartPoint,
  type UsageTrendTokenTickFormatter,
} from "@/components/usage/UsageTrendChart";
import type { AppId } from "@/lib/api/types";
import { cn } from "@/lib/utils";

export type UsageCardId = "cost" | "requests" | "tokens" | "hitRate";
export type BreakdownDimension = "app" | "provider" | "model";

export interface TrayUsagePoint extends UsageTrendChartPoint {
  costShown: number | null;
}

export const CARD_SPRING = {
  type: "spring",
  stiffness: 380,
  damping: 32,
  mass: 0.85,
} as const;

export const PILL_SPRING = {
  type: "spring",
  stiffness: 460,
  damping: 34,
} as const;

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
  size = "md",
  className,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  label: string;
  size?: "sm" | "md";
  className?: string;
}) {
  const layoutId = `tray-seg-${useId()}`;
  return (
    <div
      role="tablist"
      aria-label={label}
      className={cn(
        "flex rounded-lg bg-black/[0.06] p-0.5 dark:bg-white/[0.08]",
        className,
      )}
    >
      {options.map((option) => {
        const active = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onChange(option.value)}
            className={cn(
              "relative flex-1 whitespace-nowrap rounded-md font-medium transition-colors",
              size === "sm"
                ? "px-1.5 py-0.5 text-[10.5px]"
                : "px-2 py-1 text-xs",
              active
                ? "text-foreground"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {active && (
              <motion.span
                layoutId={layoutId}
                transition={PILL_SPRING}
                aria-hidden="true"
                className="absolute inset-0 rounded-md bg-white shadow-sm dark:bg-white/[0.16]"
              />
            )}
            <span className="relative z-10">{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}

/** 卡片内迷你柱图：最多取最近 10 个桶，突出峰值或当前联动选中的桶 */
export function MiniCostBars({
  points,
  selectedXKey,
}: {
  points: TrayUsagePoint[];
  selectedXKey: string | null;
}) {
  const slice = useMemo(() => {
    if (points.length === 0) return [];
    const recent = points.slice(-10);
    const values = recent.map((p) => Math.max(0, p.cost ?? 0));
    const max = Math.max(...values, 0);
    let peakIndex = -1;
    if (max > 0) {
      peakIndex = values.indexOf(max);
    }
    return recent.map((p, idx) => ({
      xKey: p.xKey,
      ratio: max > 0 ? Math.max(0.14, values[idx] / max) : 0.14,
      isPeak: idx === peakIndex,
      isSelected: selectedXKey === p.xKey,
    }));
  }, [points, selectedXKey]);

  if (slice.length === 0) return null;

  return (
    <div
      aria-hidden="true"
      className="flex h-6 shrink-0 items-end gap-[2.5px] pb-0.5"
    >
      {slice.map((bar) => {
        const highlight = selectedXKey ? bar.isSelected : bar.isPeak;
        return (
          <span
            key={bar.xKey}
            className="flex h-full flex-col items-center justify-end gap-[2px]"
          >
            {highlight && (
              <span className="h-1 w-1 rounded-full bg-sky-500 dark:bg-sky-400" />
            )}
            <span
              className={cn(
                "w-[3.5px] rounded-full transition-[height,background-color,opacity] duration-300 ease-out",
                highlight
                  ? "bg-sky-500 dark:bg-sky-400"
                  : selectedXKey
                    ? "bg-black/15 opacity-60 dark:bg-white/20"
                    : "bg-sky-500/35 dark:bg-sky-400/35",
              )}
              style={{ height: `${Math.round(bar.ratio * 18)}px` }}
            />
          </span>
        );
      })}
    </div>
  );
}

/** 卡片内迷你趋势折线/面积图 */
export function MiniRequestsSpark({
  points,
  selectedXKey,
}: {
  points: TrayUsagePoint[];
  selectedXKey: string | null;
}) {
  const gradientId = useId();
  const geometry = useMemo(() => {
    const recent = points.slice(-12);
    if (recent.length === 0) return null;
    const width = 46;
    const height = 22;
    const padX = 2;
    const padY = 3;
    const values = recent.map((p) => p.requests);
    const max = Math.max(...values, 1);
    const coords = recent.map((p, i) => {
      const x =
        recent.length === 1
          ? width / 2
          : padX + (i / (recent.length - 1)) * (width - padX * 2);
      const y = height - padY - (p.requests / max) * (height - padY * 2);
      return { x, y, xKey: p.xKey };
    });
    const linePath = coords
      .map(
        (c, i) => `${i === 0 ? "M" : "L"}${c.x.toFixed(1)},${c.y.toFixed(1)}`,
      )
      .join(" ");
    const areaPath =
      coords.length === 1
        ? ""
        : `${linePath} L${coords[coords.length - 1].x.toFixed(1)},${height} L${coords[0].x.toFixed(1)},${height} Z`;
    const activeCoord =
      (selectedXKey && coords.find((c) => c.xKey === selectedXKey)) ||
      coords[coords.length - 1];
    return { width, height, linePath, areaPath, activeCoord };
  }, [points, selectedXKey]);

  if (!geometry) return null;

  return (
    <svg
      aria-hidden="true"
      width={geometry.width}
      height={geometry.height}
      viewBox={`0 0 ${geometry.width} ${geometry.height}`}
      className="shrink-0 overflow-visible"
    >
      <defs>
        <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#38bdf8" stopOpacity={0.35} />
          <stop offset="100%" stopColor="#38bdf8" stopOpacity={0} />
        </linearGradient>
      </defs>
      {geometry.areaPath && (
        <path d={geometry.areaPath} fill={`url(#${gradientId})`} />
      )}
      <path
        d={geometry.linePath}
        fill="none"
        stroke="#38bdf8"
        strokeWidth={1.6}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      {geometry.activeCoord && (
        <circle
          cx={geometry.activeCoord.x}
          cy={geometry.activeCoord.y}
          r={2.2}
          fill="#0ea5e9"
          stroke="white"
          strokeWidth={1}
        />
      )}
    </svg>
  );
}

/** Token 构成比例条（输入 / 输出 / 缓存写入 / 缓存命中） */
export const TOKEN_SEGMENTS = [
  { key: "input", colorClass: "bg-sky-500", dotClass: "bg-sky-500" },
  { key: "output", colorClass: "bg-violet-500", dotClass: "bg-violet-500" },
  { key: "cacheWrite", colorClass: "bg-amber-500", dotClass: "bg-amber-500" },
  {
    key: "cacheRead",
    colorClass: "bg-emerald-500",
    dotClass: "bg-emerald-500",
  },
] as const;

export function StackedTokenBar({
  input,
  output,
  cacheWrite,
  cacheRead,
}: {
  input: number;
  output: number;
  cacheWrite: number;
  cacheRead: number;
}) {
  const total = input + output + cacheWrite + cacheRead;
  const parts = [
    { key: "input", value: input, color: "bg-sky-500" },
    { key: "output", value: output, color: "bg-violet-500" },
    { key: "cacheWrite", value: cacheWrite, color: "bg-amber-500" },
    { key: "cacheRead", value: cacheRead, color: "bg-emerald-500" },
  ].filter((p) => p.value > 0);

  return (
    <div
      aria-hidden="true"
      className="flex h-1.5 w-full overflow-hidden rounded-full bg-black/[0.08] dark:bg-white/[0.1]"
    >
      {total > 0 &&
        parts.map((part) => (
          <span
            key={part.key}
            className={cn(
              "h-full transition-[width] duration-300 ease-out first:rounded-l-full last:rounded-r-full",
              part.color,
            )}
            style={{ width: `${Math.max(2, (part.value / total) * 100)}%` }}
          />
        ))}
    </div>
  );
}

/** 缓存命中率环形进度指示 */
export function HitRateRing({ rate }: { rate: number }) {
  const clamped = Math.max(0, Math.min(1, rate));
  const size = 26;
  const stroke = 3;
  const radius = (size - stroke) / 2;
  const circumference = 2 * Math.PI * radius;
  const offset = circumference * (1 - clamped);
  const strokeColor =
    clamped >= 0.7 ? "#10b981" : clamped >= 0.35 ? "#38bdf8" : "#f59e0b";

  return (
    <svg
      aria-hidden="true"
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      className="shrink-0 -rotate-90"
    >
      <circle
        cx={size / 2}
        cy={size / 2}
        r={radius}
        fill="none"
        stroke="currentColor"
        strokeWidth={stroke}
        className="text-black/[0.08] dark:text-white/[0.12]"
      />
      <circle
        cx={size / 2}
        cy={size / 2}
        r={radius}
        fill="none"
        stroke={strokeColor}
        strokeWidth={stroke}
        strokeLinecap="round"
        strokeDasharray={circumference}
        strokeDashoffset={offset}
        className="transition-[stroke-dashoffset,stroke] duration-300 ease-out"
      />
    </svg>
  );
}

/** 2×2 指标卡片：数值 + 统计图形 + 点击联动指标 + 单卡下钻按钮 */
export function MetricBentoCard({
  id,
  label,
  value,
  detail,
  graphic,
  footerGraphic,
  selected,
  expandLabel,
  onSelectMetric,
  onDrillDown,
}: {
  id: UsageCardId;
  label: string;
  value: string;
  detail?: string;
  graphic?: React.ReactNode;
  footerGraphic?: React.ReactNode;
  selected: boolean;
  expandLabel: string;
  onSelectMetric: () => void;
  onDrillDown: () => void;
}) {
  return (
    <motion.div
      layoutId={`tray-usage-card-${id}`}
      transition={CARD_SPRING}
      className={cn(
        "group relative min-w-0 rounded-lg bg-black/[0.04] px-2.5 py-1.5 transition-[background-color,box-shadow] dark:bg-white/[0.06]",
        selected
          ? "bg-black/[0.06] ring-1 ring-sky-500/45 dark:bg-white/[0.09] dark:ring-sky-400/45"
          : "hover:bg-black/[0.06] dark:hover:bg-white/[0.08]",
      )}
    >
      <button
        type="button"
        onClick={() => {
          if (selected) onDrillDown();
          else onSelectMetric();
        }}
        className="flex w-full min-w-0 flex-col text-left outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60 rounded"
      >
        <div className="flex w-full items-center gap-1 pr-5">
          <span
            className={cn(
              "truncate text-[11px] transition-colors",
              selected
                ? "font-medium text-foreground"
                : "text-muted-foreground",
            )}
          >
            {label}
          </span>
        </div>
        <div className="mt-0.5 flex w-full items-end justify-between gap-1.5">
          <div className="min-w-0 flex-1 truncate text-[17px] font-semibold leading-6 tabular-nums">
            {value}
          </div>
          {graphic}
        </div>
        {footerGraphic && <div className="mt-1 w-full">{footerGraphic}</div>}
        {detail && (
          <div className="mt-0.5 w-full truncate text-[10.5px] tabular-nums text-muted-foreground">
            {detail}
          </div>
        )}
      </button>
      <button
        type="button"
        aria-label={`${expandLabel}: ${label}`}
        title={expandLabel}
        onClick={(event) => {
          event.stopPropagation();
          onDrillDown();
        }}
        className={cn(
          "absolute right-1.5 top-1.5 flex h-5 w-5 items-center justify-center rounded-md text-muted-foreground transition-all",
          "opacity-55 hover:bg-black/[0.08] hover:text-foreground hover:opacity-100 dark:hover:bg-white/[0.12]",
          "focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60",
        )}
      >
        <Maximize2 className="h-3 w-3" />
      </button>
    </motion.div>
  );
}

/** 趋势图组件（概览图与下钻详情图共用），支持点击柱子/点联动筛选 */
export function UsageTrendGraphic({
  chartType,
  chartData,
  metric,
  selectedXKey,
  onSelectPoint,
  currencySymbol,
  tickFormatter,
  formatValue,
  hitRateFloor,
  heightClass = "h-[120px]",
}: {
  chartType: "bar" | "area";
  chartData: TrayUsagePoint[];
  metric: UsageCardId;
  selectedXKey: string | null;
  onSelectPoint: (xKey: string) => void;
  currencySymbol: string;
  tickFormatter: UsageTrendTokenTickFormatter;
  formatValue: (point: TrayUsagePoint, metric: UsageCardId) => string;
  hitRateFloor: number;
  heightClass?: string;
}) {
  const Chart = chartType === "bar" ? BarChart : AreaChart;
  const dataKey =
    metric === "cost"
      ? "costShown"
      : metric === "requests"
        ? "requests"
        : metric === "hitRate"
          ? "hitRate"
          : "tokens";

  const handleChartClick = (state: unknown) => {
    const activeLabel = (state as { activeLabel?: string } | null)?.activeLabel;
    if (typeof activeLabel === "string" && activeLabel) {
      onSelectPoint(activeLabel);
    }
  };

  return (
    <div className={heightClass}>
      <ResponsiveContainer width="100%" height="100%">
        <Chart
          data={chartData}
          margin={{ top: 4, right: 4, bottom: 0, left: 0 }}
          onClick={handleChartClick}
          className="cursor-pointer"
        >
          <XAxis
            dataKey="xKey"
            tickFormatter={(key: string) =>
              chartData.find((p) => p.xKey === key)?.label ?? key
            }
            tick={{ fill: "currentColor", fontSize: 10, opacity: 0.55 }}
            tickLine={false}
            axisLine={false}
            interval="preserveStartEnd"
            minTickGap={24}
          />
          <YAxis
            width="auto"
            tick={{ fill: "currentColor", fontSize: 10, opacity: 0.55 }}
            tickLine={false}
            axisLine={false}
            tickCount={3}
            domain={metric === "hitRate" ? [hitRateFloor, 1] : undefined}
            tickFormatter={(value: unknown) => {
              if (metric === "hitRate") {
                return `${Math.round((Number(value) || 0) * 100)}%`;
              }
              return metric === "cost"
                ? `${currencySymbol}${formatUsageTrendTokenTickLabel(value, tickFormatter)}`
                : formatUsageTrendTokenTickLabel(value, tickFormatter);
            }}
          />
          {selectedXKey && chartType === "area" && (
            <ReferenceLine
              x={selectedXKey}
              stroke="#0ea5e9"
              strokeDasharray="3 3"
              strokeOpacity={0.75}
            />
          )}
          <Tooltip
            cursor={
              chartType === "bar"
                ? { fill: "rgba(127,127,127,0.12)" }
                : {
                    stroke: "rgba(127,127,127,0.5)",
                    strokeDasharray: "3 3",
                  }
            }
            content={({ active, payload }) => {
              const point = payload?.[0]?.payload as TrayUsagePoint | undefined;
              if (!active || !point) return null;
              return (
                <div className="rounded-md bg-popover px-2 py-1 text-[11px] text-popover-foreground shadow-md">
                  <div className="font-medium">
                    {point.tooltipLabel ?? point.label}
                  </div>
                  <div className="tabular-nums">
                    {formatValue(point, metric)}
                  </div>
                </div>
              );
            }}
          />
          {chartType === "bar" ? (
            <Bar
              dataKey={dataKey}
              fill="#38bdf8"
              radius={[3, 3, 0, 0]}
              maxBarSize={18}
              isAnimationActive={false}
              onClick={(entry: unknown) => {
                const xKey =
                  (entry as { xKey?: string; payload?: { xKey?: string } })
                    ?.xKey ??
                  (entry as { payload?: { xKey?: string } })?.payload?.xKey;
                if (typeof xKey === "string") onSelectPoint(xKey);
              }}
            >
              {chartData.map((point) => {
                const isSelected = selectedXKey === point.xKey;
                const dimmed = selectedXKey != null && !isSelected;
                return (
                  <Cell
                    key={point.xKey}
                    fill={isSelected ? "#0ea5e9" : "#38bdf8"}
                    fillOpacity={dimmed ? 0.35 : 0.95}
                  />
                );
              })}
            </Bar>
          ) : (
            <Area
              type="monotone"
              dataKey={dataKey}
              stroke="#38bdf8"
              strokeWidth={2}
              fill="#38bdf8"
              fillOpacity={0.16}
              dot={(props: {
                cx?: number;
                cy?: number;
                payload?: TrayUsagePoint;
                index?: number;
              }) => {
                const xKey = props.payload?.xKey;
                const isSelected =
                  selectedXKey != null && xKey === selectedXKey;
                if (!isSelected && chartData.length > 1)
                  return <g key={props.index} />;
                return (
                  <circle
                    key={xKey ?? props.index}
                    cx={props.cx}
                    cy={props.cy}
                    r={isSelected ? 4 : 3}
                    fill="#0ea5e9"
                    stroke="white"
                    strokeWidth={1.2}
                  />
                );
              }}
              activeDot={{ r: 3.5 }}
              isAnimationActive={false}
            />
          )}
        </Chart>
      </ResponsiveContainer>
    </div>
  );
}

export interface BreakdownItem {
  key: string;
  label: string;
  appId?: AppId;
  iconName?: string;
  valueText: string;
  share: number;
  active: boolean;
  onToggle: () => void;
}

export interface DetailSubMetric {
  label: string;
  value: string;
  dotClass: string;
}

/** 单张卡片下钻后的详情视图，通过 shared layoutId 从网格卡片平滑展开 */
export function UsageCardDetailView({
  cardId,
  title,
  icon,
  heroValue,
  heroSubtitle,
  subMetrics,
  chart,
  dimension,
  onDimensionChange,
  dimensionOptions,
  dimensionLabel,
  breakdownItems,
  noBreakdownText,
  insight,
  backLabel,
  onBack,
}: {
  cardId: UsageCardId;
  title: string;
  icon: React.ReactNode;
  heroValue: string;
  heroSubtitle: string;
  subMetrics: DetailSubMetric[];
  chart: React.ReactNode;
  dimension: BreakdownDimension;
  onDimensionChange: (dim: BreakdownDimension) => void;
  dimensionOptions: { value: BreakdownDimension; label: string }[];
  dimensionLabel: string;
  breakdownItems: BreakdownItem[];
  noBreakdownText: string;
  insight: string | null;
  backLabel: string;
  onBack: () => void;
}) {
  return (
    <motion.div
      layoutId={`tray-usage-card-${cardId}`}
      transition={CARD_SPRING}
      className="flex flex-col gap-2 rounded-lg bg-black/[0.04] p-2.5 dark:bg-white/[0.06]"
    >
      {/* 顶栏：图标 + 标题 + 收起按钮 */}
      <div className="flex items-center gap-1.5">
        <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-black/[0.06] text-foreground dark:bg-white/[0.1]">
          {icon}
        </span>
        <span className="truncate text-xs font-semibold">{title}</span>
        <button
          type="button"
          aria-label={backLabel}
          title={backLabel}
          onClick={onBack}
          className="ms-auto flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-black/[0.08] hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60 dark:hover:bg-white/[0.12]"
        >
          <Minimize2 className="h-3.5 w-3.5" />
        </button>
      </div>

      {/* 主数值 + 彩点细分指标（参考视频中 Sleep 卡片的左右分布） */}
      <motion.div
        initial={{ opacity: 0, y: 4 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.18, delay: 0.04 }}
        className="flex items-end justify-between gap-2 px-0.5"
      >
        <div className="min-w-0">
          <div className="truncate text-[22px] font-semibold leading-7 tabular-nums">
            {heroValue}
          </div>
          <div className="truncate text-[10.5px] text-muted-foreground">
            {heroSubtitle}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2.5">
          {subMetrics.map((item) => (
            <div key={item.label} className="min-w-0 text-right">
              <div className="flex items-center justify-end gap-1 text-[10px] text-muted-foreground">
                <span
                  aria-hidden="true"
                  className={cn("h-1.5 w-1.5 rounded-full", item.dotClass)}
                />
                <span className="truncate">{item.label}</span>
              </div>
              <div className="text-xs font-semibold tabular-nums">
                {item.value}
              </div>
            </div>
          ))}
        </div>
      </motion.div>

      {/* 详图 */}
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: 0.2, delay: 0.06 }}
      >
        {chart}
      </motion.div>

      {/* 维度下钻细分：按应用 / 按供应商 / 按模型，点击即可联动筛选 */}
      <motion.div
        initial={{ opacity: 0, y: 4 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.18, delay: 0.08 }}
        className="flex flex-col gap-1.5 border-t border-black/[0.06] pt-2 dark:border-white/[0.08]"
      >
        <Segmented
          size="sm"
          label={dimensionLabel}
          value={dimension}
          onChange={onDimensionChange}
          options={dimensionOptions}
        />
        {breakdownItems.length === 0 ? (
          <div className="py-2 text-center text-[11px] text-muted-foreground">
            {noBreakdownText}
          </div>
        ) : (
          <div className="flex flex-col gap-1">
            {breakdownItems.map((item) => (
              <button
                key={item.key}
                type="button"
                onClick={item.onToggle}
                className={cn(
                  "group relative flex w-full flex-col gap-0.5 overflow-hidden rounded-md px-2 py-1 text-left transition-colors",
                  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60",
                  item.active
                    ? "bg-sky-500/15 ring-1 ring-sky-500/45 dark:bg-sky-400/15 dark:ring-sky-400/45"
                    : "hover:bg-black/[0.05] dark:hover:bg-white/[0.07]",
                )}
              >
                <div className="flex w-full items-center gap-1.5 text-[11px]">
                  {item.appId ? (
                    <AppGlyph
                      app={item.appId}
                      size={13}
                      badgeClassName="bg-transparent"
                    />
                  ) : item.iconName !== undefined ? (
                    <ProviderIcon
                      icon={item.iconName}
                      name={item.label}
                      size={13}
                    />
                  ) : null}
                  <span className="min-w-0 flex-1 truncate font-medium">
                    {item.label}
                  </span>
                  <span className="shrink-0 tabular-nums text-muted-foreground">
                    {item.valueText}
                  </span>
                </div>
                <div className="h-[3px] w-full overflow-hidden rounded-full bg-black/[0.06] dark:bg-white/[0.08]">
                  <div
                    className={cn(
                      "h-full rounded-full transition-[width] duration-300 ease-out",
                      item.active
                        ? "bg-sky-500 dark:bg-sky-400"
                        : "bg-sky-500/60 dark:bg-sky-400/60",
                    )}
                    style={{
                      width: `${Math.max(4, Math.min(100, Math.round(item.share * 100)))}%`,
                    }}
                  />
                </div>
              </button>
            ))}
          </div>
        )}
      </motion.div>

      {/* 底部洞察提示条 */}
      {insight && (
        <motion.div
          initial={{ opacity: 0, y: 3 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.18, delay: 0.1 }}
          className="truncate rounded-md bg-black/[0.05] px-2.5 py-1.5 text-[10.5px] text-muted-foreground dark:bg-white/[0.07]"
          title={insight}
        >
          {insight}
        </motion.div>
      )}
    </motion.div>
  );
}

/**
 * 横向应用筛选条：支持桌面鼠标滚轮（竖向滚轮映射为横向滚动）、拖拽滚动，
 * 以及溢出时显示左右滚动箭头按钮。
 */
export function AppFilterStrip({
  label,
  allLabel,
  items,
  selectedApp,
  onSelectApp,
}: {
  label: string;
  allLabel: string;
  items: Array<{ appType: string; name: string }>;
  selectedApp: string | undefined;
  onSelectApp: (appType: string | undefined) => void;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [canScrollLeft, setCanScrollLeft] = useState(false);
  const [canScrollRight, setCanScrollRight] = useState(false);
  const dragState = useRef<{
    active: boolean;
    startX: number;
    startScrollLeft: number;
    moved: boolean;
  }>({ active: false, startX: 0, startScrollLeft: 0, moved: false });
  const suppressNextClick = useRef(false);

  const syncScrollState = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const maxScroll = el.scrollWidth - el.clientWidth;
    setCanScrollLeft(el.scrollLeft > 2);
    setCanScrollRight(maxScroll - el.scrollLeft > 2);
  }, []);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    syncScrollState();

    const onWheel = (event: WheelEvent) => {
      if (el.scrollWidth <= el.clientWidth) return;
      if (
        Math.abs(event.deltaY) >= Math.abs(event.deltaX) &&
        event.deltaY !== 0
      ) {
        event.preventDefault();
        el.scrollLeft += event.deltaY;
        syncScrollState();
      }
    };

    el.addEventListener("wheel", onWheel, { passive: false });
    const observer =
      typeof ResizeObserver !== "undefined"
        ? new ResizeObserver(syncScrollState)
        : null;
    observer?.observe(el);

    return () => {
      el.removeEventListener("wheel", onWheel);
      observer?.disconnect();
    };
  }, [items, syncScrollState]);

  const scrollByStep = (direction: -1 | 1) => {
    const el = scrollRef.current;
    if (!el) return;
    const delta = direction * Math.max(96, Math.round(el.clientWidth * 0.65));
    if (typeof el.scrollBy === "function") {
      el.scrollBy({ left: delta, behavior: "smooth" });
    } else {
      el.scrollLeft += delta;
    }
    // 立即兜底同步一次状态（jsdom 等无 smooth 滚动事件环境）
    window.setTimeout(syncScrollState, 0);
  };

  const handlePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const el = scrollRef.current;
    if (!el || el.scrollWidth <= el.clientWidth) return;
    dragState.current = {
      active: true,
      startX: event.clientX,
      startScrollLeft: el.scrollLeft,
      moved: false,
    };
  };

  const handlePointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragState.current;
    const el = scrollRef.current;
    if (!drag.active || !el) return;
    const dx = event.clientX - drag.startX;
    if (!drag.moved && Math.abs(dx) > 4) {
      drag.moved = true;
    }
    if (drag.moved) {
      el.scrollLeft = drag.startScrollLeft - dx;
      syncScrollState();
    }
  };

  const finishDrag = () => {
    if (dragState.current.moved) {
      suppressNextClick.current = true;
      window.setTimeout(() => {
        suppressNextClick.current = false;
      }, 0);
    }
    dragState.current.active = false;
    dragState.current.moved = false;
  };

  const handleChipClick = (
    event: React.MouseEvent<HTMLButtonElement>,
    nextApp: string | undefined,
  ) => {
    if (suppressNextClick.current) {
      event.preventDefault();
      return;
    }
    event.currentTarget.scrollIntoView?.({
      inline: "nearest",
      block: "nearest",
      behavior: "smooth",
    });
    onSelectApp(nextApp);
  };

  return (
    <div className="flex min-w-0 flex-1 items-center gap-0.5">
      {canScrollLeft && (
        <button
          type="button"
          aria-label="Scroll apps left"
          onClick={() => scrollByStep(-1)}
          className="flex h-5 w-4 shrink-0 items-center justify-center rounded bg-black/[0.05] text-muted-foreground transition-colors hover:bg-black/[0.1] hover:text-foreground dark:bg-white/[0.08] dark:hover:bg-white/[0.14]"
        >
          <ChevronLeft className="h-3 w-3" />
        </button>
      )}
      <div
        ref={scrollRef}
        role="group"
        aria-label={label}
        onScroll={syncScrollState}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={finishDrag}
        onPointerCancel={finishDrag}
        className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto no-scrollbar"
      >
        <button
          type="button"
          onClick={(event) => handleChipClick(event, undefined)}
          className={cn(
            "shrink-0 rounded-md px-2 py-0.5 text-[11px] font-medium transition-colors",
            !selectedApp
              ? "bg-white text-foreground shadow-sm dark:bg-white/[0.16]"
              : "bg-black/[0.04] text-muted-foreground hover:text-foreground dark:bg-white/[0.06]",
          )}
        >
          {allLabel}
        </button>
        {items.map((entry) => {
          const active = selectedApp === entry.appType;
          return (
            <button
              key={entry.appType}
              type="button"
              onClick={(event) =>
                handleChipClick(event, active ? undefined : entry.appType)
              }
              className={cn(
                "flex shrink-0 items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium transition-colors",
                active
                  ? "bg-white text-foreground shadow-sm ring-1 ring-sky-500/40 dark:bg-white/[0.16] dark:ring-sky-400/40"
                  : "bg-black/[0.04] text-muted-foreground hover:text-foreground dark:bg-white/[0.06]",
              )}
            >
              <AppGlyph
                app={entry.appType as AppId}
                size={12}
                badgeClassName="bg-transparent"
              />
              <span className="max-w-[76px] truncate">{entry.name}</span>
            </button>
          );
        })}
      </div>
      {canScrollRight && (
        <button
          type="button"
          aria-label="Scroll apps right"
          onClick={() => scrollByStep(1)}
          className="flex h-5 w-4 shrink-0 items-center justify-center rounded bg-black/[0.05] text-muted-foreground transition-colors hover:bg-black/[0.1] hover:text-foreground dark:bg-white/[0.08] dark:hover:bg-white/[0.14]"
        >
          <ChevronRight className="h-3 w-3" />
        </button>
      )}
    </div>
  );
}
