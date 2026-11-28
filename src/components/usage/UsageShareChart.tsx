import { useMemo, useState } from "react";
import { Cell, Pie, PieChart, ResponsiveContainer, Tooltip } from "recharts";
import { cn } from "@/lib/utils";

export interface UsageShareDatum {
  key: string;
  label: string;
  value: number;
  detail?: string;
}

const PALETTE = [
  "var(--chart-1)",
  "var(--chart-2)",
  "var(--chart-3)",
  "var(--chart-4)",
  "var(--chart-5)",
  "var(--chart-6)",
];

interface UsageShareChartProps {
  title: string;
  ariaLabel: string;
  data: UsageShareDatum[];
  formatValue: (value: number) => string;
  emptyLabel: string;
  totalLabel: string;
  otherLabel: string;
  selectedKey?: string | null;
  onSelect?: (key: string) => void;
  maxSlices?: number;
  /** 缓存视图中零输入/缺失数据的供应商仍可选择。 */
  includeZeroValues?: boolean;
  className?: string;
}

/** 用量占比环形图 + 图例；超过 maxSlices 的尾部合并为「其他」。 */
export function UsageShareChart({
  title,
  ariaLabel,
  data,
  formatValue,
  emptyLabel,
  totalLabel,
  otherLabel,
  selectedKey,
  onSelect,
  maxSlices = 6,
  includeZeroValues = false,
  className,
}: UsageShareChartProps) {
  const slices = useMemo(() => {
    const positive = data
      .filter(
        (item) => item.value > 0 || (includeZeroValues && item.value === 0),
      )
      .sort((a, b) => b.value - a.value);
    if (positive.length <= maxSlices) return positive;
    const head = positive.slice(0, maxSlices - 1);
    const rest = positive.slice(maxSlices - 1);
    const restValue = rest.reduce((sum, item) => sum + item.value, 0);
    return [...head, { key: "__other__", label: otherLabel, value: restValue }];
  }, [data, maxSlices, otherLabel, includeZeroValues]);

  const total = slices.reduce((sum, item) => sum + item.value, 0);
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);

  const renderTooltip = ({ active, payload }: any) => {
    if (!active || !payload?.length) return null;
    const datum = payload[0]?.payload as UsageShareDatum | undefined;
    if (!datum) return null;
    return (
      <div className="pointer-events-none rounded-[8px] border border-border bg-surface px-3 py-2 text-caption text-fg-1 shadow-v7-md">
        <p className="m-0 mb-1 max-w-[240px] truncate font-semibold">
          {datum.label}
        </p>
        <p className="m-0 tabular-nums text-fg-2">{formatValue(datum.value)}</p>
        {datum.detail ? (
          <p className="m-0 mt-0.5 text-badge text-fg-3">{datum.detail}</p>
        ) : null}
      </div>
    );
  };

  return (
    <section
      aria-label={ariaLabel}
      className={cn("flex min-w-0 flex-col gap-2", className)}
    >
      <h3 className="m-0 text-caption font-semibold text-fg-2">{title}</h3>
      {slices.length === 0 ? (
        <p className="m-0 py-10 text-center text-caption text-fg-3">
          {emptyLabel}
        </p>
      ) : (
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
          <div className="relative mx-auto h-36 w-36 shrink-0">
            <ResponsiveContainer width="100%" height="100%">
              <PieChart>
                <Pie
                  // Recharts v3 的 ChartDataInput 要求索引签名，这里传入结构化的 slice。
                  data={slices as unknown as Array<Record<string, unknown>>}
                  dataKey="value"
                  nameKey="label"
                  innerRadius="62%"
                  outerRadius="100%"
                  paddingAngle={1.5}
                  stroke="none"
                  isAnimationActive={false}
                  onMouseEnter={(_data, index) => setHoveredIndex(index)}
                  onMouseLeave={() => setHoveredIndex(null)}
                >
                  {slices.map((item, index) => (
                    <Cell
                      key={item.key}
                      fill={PALETTE[index % PALETTE.length]}
                    />
                  ))}
                </Pie>
                <Tooltip content={renderTooltip} />
              </PieChart>
            </ResponsiveContainer>
            <div
              className={cn(
                "pointer-events-none absolute inset-0 flex flex-col items-center justify-center text-center transition-opacity duration-150",
                hoveredIndex != null && "opacity-0",
              )}
            >
              <span className="text-badge text-fg-2">{totalLabel}</span>
              <span
                className="max-w-[92px] truncate text-caption font-semibold tabular-nums text-fg-1"
                title={formatValue(total)}
              >
                {formatValue(total)}
              </span>
            </div>
          </div>
          <ul
            tabIndex={includeZeroValues ? 0 : undefined}
            className={cn(
              "m-0 flex min-w-0 flex-1 list-none flex-col gap-0.5 p-0",
              includeZeroValues &&
                "max-h-80 overflow-y-auto focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
            )}
          >
            {slices.map((item, index) => {
              const percent = total > 0 ? (item.value / total) * 100 : 0;
              const selected = selectedKey === item.key;
              const content = (
                <>
                  <span
                    aria-hidden="true"
                    className="size-2 shrink-0 rounded-[2px]"
                    style={{ background: PALETTE[index % PALETTE.length] }}
                  />
                  <span className="min-w-0 flex-1">
                    <span
                      className="block truncate text-fg-1"
                      title={item.label}
                    >
                      {item.label}
                    </span>
                    {item.detail ? (
                      <span className="block truncate text-badge text-fg-3">
                        {item.detail}
                      </span>
                    ) : null}
                  </span>
                  <span className="shrink-0 tabular-nums text-fg-1">
                    {formatValue(item.value)}
                  </span>
                  <span className="w-12 shrink-0 text-end tabular-nums text-fg-2">
                    {percent.toFixed(1)}%
                  </span>
                </>
              );
              return (
                <li key={item.key}>
                  {onSelect && item.key !== "__other__" ? (
                    <button
                      type="button"
                      aria-pressed={selected}
                      onClick={() => onSelect(item.key)}
                      className={cn(
                        "flex w-full items-center gap-2 rounded-[4px] px-1 py-0.5 text-start text-caption transition-colors hover:bg-subtle focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                        selected && "bg-subtle",
                      )}
                    >
                      {content}
                    </button>
                  ) : (
                    <div className="flex items-center gap-2 px-1 py-0.5 text-caption">
                      {content}
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </section>
  );
}
