import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  Cell,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  type PieLabelRenderProps,
} from "recharts";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  fmtInt,
  fmtUsd,
  formatTokensShort,
  getLocaleFromLanguage,
  getResolvedLang,
} from "./format";
import {
  formatModelShare,
  type ModelDistribution,
  type ModelDistributionRow,
} from "./modelDistribution";

interface ModelUsageDistributionProps {
  distribution: ModelDistribution;
  hiddenModels: ReadonlySet<string>;
  onToggleModel: (model: string) => void;
  onShowAll: () => void;
}

type Metric = "tokens" | "cost";

function getShare(row: ModelDistributionRow, metric: Metric): number | null {
  return metric === "tokens" ? row.tokenShare : row.costShare;
}

function compactModelLabel(model: string): string {
  const characters = Array.from(model);
  return characters.length > 10 ? characters.slice(0, 9).join("") + "…" : model;
}

interface LabelGeometry {
  x: number;
  y: number;
  width: number;
  textAnchor: "start" | "middle" | "end";
}

function getApproximateLabelWidth(
  row: ModelDistributionRow,
  metric: Metric,
): number {
  const label =
    compactModelLabel(row.model) +
    " " +
    formatModelShare(getShare(row, metric));
  return Array.from(label).length * 6;
}

function getEstimatedLabelPositions(
  rows: ModelDistributionRow[],
  metric: Metric,
  cx: number,
  cy: number,
  outerRadius: number,
): Map<string, Pick<LabelGeometry, "x" | "y" | "textAnchor">> {
  let accumulatedShare = 0;
  const labelRadius = outerRadius + 20;
  const positions = new Map<
    string,
    Pick<LabelGeometry, "x" | "y" | "textAnchor">
  >();

  for (const row of rows) {
    const share = getShare(row, metric) ?? 0;
    const midAngle = (accumulatedShare + share / 2) * 360;
    accumulatedShare += share;

    const radians = (-midAngle * Math.PI) / 180;
    const x = cx + Math.cos(radians) * labelRadius;
    const y = cy + Math.sin(radians) * labelRadius;
    positions.set(row.model, {
      x,
      y,
      textAnchor: x >= cx ? "start" : "end",
    });
  }

  return positions;
}

function labelsOverlap(left: LabelGeometry, right: LabelGeometry): boolean {
  const bounds = (label: LabelGeometry) => {
    if (label.textAnchor === "end") {
      return { start: label.x - label.width, end: label.x };
    }
    if (label.textAnchor === "middle") {
      return {
        start: label.x - label.width / 2,
        end: label.x + label.width / 2,
      };
    }
    return { start: label.x, end: label.x + label.width };
  };
  const leftBounds = bounds(left);
  const rightBounds = bounds(right);

  return (
    Math.abs(left.y - right.y) < 14 &&
    leftBounds.start < rightBounds.end + 4 &&
    leftBounds.end + 4 > rightBounds.start
  );
}
function DistributionTooltip({
  active,
  payload,
  metric,
  hiddenModels,
}: {
  active?: boolean;
  payload?: ReadonlyArray<{ payload?: unknown }>;
  metric: Metric;
  hiddenModels: ReadonlySet<string>;
}) {
  const { t, i18n } = useTranslation();
  const row = payload?.[0]?.payload as ModelDistributionRow | undefined;

  if (!active || !row || hiddenModels.has(row.model)) return null;

  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const valueLabel =
    metric === "tokens"
      ? t("usage.tokens", "Tokens")
      : t("usage.modelDistribution.estimatedCost");
  const value =
    metric === "tokens" ? fmtInt(row.tokenValue, locale) : fmtUsd(row.cost, 4);
  const share = formatModelShare(getShare(row, metric));

  return (
    <div className="max-w-72 rounded-md border border-border bg-popover px-3 py-2 text-popover-foreground shadow-md">
      <p className="mb-1 break-all font-medium">{row.model}</p>
      <p className="text-sm text-muted-foreground">
        {valueLabel}: <span className="text-foreground">{value}</span>
      </p>
      <p className="text-sm text-muted-foreground">
        {t(
          metric === "tokens"
            ? "usage.modelDistribution.tokenShare"
            : "usage.modelDistribution.costShare",
        )}
        : <span className="text-foreground">{share}</span>
      </p>
    </div>
  );
}

function DistributionChartCard({
  metric,
  distribution,
  hiddenModels,
}: {
  metric: Metric;
  distribution: ModelDistribution;
  hiddenModels: ReadonlySet<string>;
}) {
  const { t, i18n } = useTranslation();
  const isTokens = metric === "tokens";
  const total = isTokens ? distribution.totalTokens : distribution.totalCost;
  const hasInvalidData = isTokens
    ? distribution.invalidTokens
    : distribution.invalidCost;
  const isZero = total === 0;
  const allHidden =
    distribution.rows.length > 0 &&
    distribution.rows.every((row) => hiddenModels.has(row.model));
  const lang = getResolvedLang(i18n);
  const locale = getLocaleFromLanguage(lang);
  const totalLabel = isTokens
    ? formatTokensShort(total, lang)
    : fmtUsd(total, 4);
  const fullTotalLabel = isTokens ? fmtInt(total, locale) : fmtUsd(total, 4);
  const title = t(
    isTokens
      ? "usage.modelDistribution.tokenTitle"
      : "usage.modelDistribution.costTitle",
  );
  const description = t(
    isTokens
      ? "usage.modelDistribution.tokenHint"
      : "usage.modelDistribution.estimatedCost",
  );
  const valueKey = isTokens ? "tokenValue" : "cost";
  const chartRows = useMemo<Array<Record<string, unknown>>>(
    () => distribution.rows.map((row) => ({ ...row })),
    [distribution.rows],
  );

  const labelCandidates = useMemo(
    () =>
      distribution.rows
        .filter(
          (row) =>
            !hiddenModels.has(row.model) &&
            (getShare(row, metric) ?? 0) >= 0.02,
        )
        .sort((left, right) => {
          const shareDifference =
            (getShare(right, metric) ?? 0) - (getShare(left, metric) ?? 0);
          if (shareDifference !== 0) return shareDifference;
          return left.model.localeCompare(right.model);
        })
        .slice(0, 6),
    [distribution.rows, hiddenModels, metric],
  );

  const renderLabel = (props: PieLabelRenderProps) => {
    const row = props.payload as ModelDistributionRow | undefined;
    const share = row ? getShare(row, metric) : null;
    if (!row || share == null) return null;

    const labelIndex = labelCandidates.findIndex(
      (candidate) => candidate.model === row.model,
    );
    const maxLabels = props.outerRadius < 80 ? 3 : 6;
    if (labelIndex < 0 || labelIndex >= maxLabels) return null;

    const textAnchor = props.textAnchor as "start" | "middle" | "end";
    const currentLabel: LabelGeometry = {
      x: props.x,
      y: props.y,
      width: getApproximateLabelWidth(row, metric),
      textAnchor,
    };
    const estimatedPositions = getEstimatedLabelPositions(
      distribution.rows,
      metric,
      props.cx,
      props.cy,
      props.outerRadius,
    );
    const collidesWithHigherPriorityLabel = labelCandidates
      .slice(0, labelIndex)
      .some((candidate) => {
        const position = estimatedPositions.get(candidate.model);
        if (!position) return false;
        return labelsOverlap(currentLabel, {
          ...position,
          width: getApproximateLabelWidth(candidate, metric),
        });
      });
    if (collidesWithHigherPriorityLabel) return null;

    return (
      <text
        x={props.x}
        y={props.y}
        fill="hsl(var(--foreground))"
        fontSize={11}
        textAnchor={textAnchor}
        dominantBaseline="central"
      >
        <title>{row.model}</title>
        {compactModelLabel(row.model) + " " + formatModelShare(share)}
      </text>
    );
  };

  return (
    <Card className="min-w-0 overflow-hidden bg-card/40">
      <CardHeader className="pb-2">
        <CardTitle className="text-base">{title}</CardTitle>
        <CardDescription className="truncate" title={description}>
          {description}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-2">
        <div className="relative h-[280px] min-w-0">
          {isZero && !hasInvalidData && (
            <div
              aria-hidden="true"
              className="pointer-events-none absolute left-1/2 top-1/2 z-0 h-40 w-40 -translate-x-1/2 -translate-y-1/2 rounded-full border-[28px] border-muted/30"
            />
          )}
          <ResponsiveContainer width="100%" height="100%">
            <PieChart margin={{ top: 8, right: 48, bottom: 8, left: 48 }}>
              {!hasInvalidData && !isZero && (
                <Pie
                  data={chartRows}
                  dataKey={valueKey}
                  nameKey="model"
                  cx="50%"
                  cy="50%"
                  innerRadius="48%"
                  outerRadius="66%"
                  paddingAngle={1}
                  label={renderLabel}
                  labelLine={false}
                  isAnimationActive={false}
                  stroke="hsl(var(--card))"
                  strokeWidth={1}
                >
                  {distribution.rows.map((row) => {
                    const hidden = hiddenModels.has(row.model);
                    return (
                      <Cell
                        key={row.model}
                        fill={hidden ? "transparent" : row.color}
                        stroke={hidden ? "none" : "hsl(var(--card))"}
                        strokeWidth={hidden ? 0 : 1}
                        pointerEvents={hidden ? "none" : "auto"}
                      />
                    );
                  })}
                </Pie>
              )}
              <Tooltip
                content={
                  <DistributionTooltip
                    metric={metric}
                    hiddenModels={hiddenModels}
                  />
                }
                cursor={false}
              />
            </PieChart>
          </ResponsiveContainer>

          <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center">
            <div className="max-w-[44%] text-center" title={fullTotalLabel}>
              <p className="truncate text-xl font-semibold tabular-nums">
                {hasInvalidData ? "—" : totalLabel}
              </p>
            </div>
          </div>
        </div>

        {(hasInvalidData || isZero || allHidden) && (
          <p
            className="min-h-5 text-center text-xs text-muted-foreground"
            role="status"
          >
            {hasInvalidData
              ? t("usage.modelDistribution.invalidData")
              : allHidden
                ? t("usage.modelDistribution.allHidden")
                : isTokens
                  ? t("usage.modelDistribution.zeroTokens")
                  : t("usage.modelDistribution.zeroCost")}
          </p>
        )}
      </CardContent>
    </Card>
  );
}

export function ModelUsageDistribution({
  distribution,
  hiddenModels,
  onToggleModel,
  onShowAll,
}: ModelUsageDistributionProps) {
  const { t } = useTranslation();
  const hasHiddenModels = distribution.rows.some((row) =>
    hiddenModels.has(row.model),
  );

  if (distribution.rows.length === 0) {
    return (
      <div className="rounded-xl border border-border/50 bg-card/40 py-12 text-center text-sm text-muted-foreground">
        {t("usage.noData")}
      </div>
    );
  }

  return (
    <section className="space-y-4" aria-label={t("usage.model")}>
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
        <DistributionChartCard
          metric="tokens"
          distribution={distribution}
          hiddenModels={hiddenModels}
        />
        <DistributionChartCard
          metric="cost"
          distribution={distribution}
          hiddenModels={hiddenModels}
        />
      </div>

      <div className="rounded-xl border border-border/50 bg-card/40 p-4">
        {hasHiddenModels && (
          <div className="mb-3 flex justify-end">
            <button
              type="button"
              className="rounded-sm text-sm font-medium text-primary underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
              onClick={onShowAll}
            >
              {t("usage.modelDistribution.showAll")}
            </button>
          </div>
        )}
        <div className="mb-1 flex justify-end gap-2 px-2 text-[10px] text-muted-foreground">
          <span className="w-16 text-right">
            {t("usage.modelDistribution.tokenShare")}
          </span>
          <span className="w-16 text-right">
            {t("usage.modelDistribution.costShare")}
          </span>
        </div>
        <div
          className="grid max-h-72 grid-cols-1 gap-1 overflow-y-auto"
          role="group"
          aria-label={t("usage.model")}
        >
          {distribution.rows.map((row) => {
            const hidden = hiddenModels.has(row.model);
            const tokenShare = formatModelShare(row.tokenShare);
            const costShare = formatModelShare(row.costShare);
            const ariaLabel =
              row.model +
              ", " +
              t("usage.modelDistribution.tokenShare") +
              ": " +
              tokenShare +
              ", " +
              t("usage.modelDistribution.costShare") +
              ": " +
              costShare;

            return (
              <button
                key={row.model}
                type="button"
                aria-label={ariaLabel}
                aria-pressed={!hidden}
                title={row.model}
                onClick={() => onToggleModel(row.model)}
                className={
                  "flex min-w-0 items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring " +
                  (hidden ? "opacity-50" : "")
                }
              >
                <span
                  aria-hidden="true"
                  className="h-2.5 w-2.5 shrink-0 rounded-full"
                  style={{ backgroundColor: row.color }}
                />
                <span className="min-w-0 flex-1 truncate font-mono text-xs">
                  {row.model}
                </span>
                <span
                  className="w-16 shrink-0 text-right text-xs tabular-nums text-muted-foreground"
                  title={t("usage.modelDistribution.tokenShare")}
                >
                  {tokenShare}
                </span>
                <span
                  className="w-16 shrink-0 text-right text-xs tabular-nums text-muted-foreground"
                  title={t("usage.modelDistribution.costShare")}
                >
                  {costShare}
                </span>
              </button>
            );
          })}
        </div>
      </div>
    </section>
  );
}
