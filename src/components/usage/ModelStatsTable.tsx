import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useModelStats } from "@/lib/query/usage";
import type { UsageRangeSelection, ModelStats } from "@/types/usage";
import { Loader2 } from "lucide-react";

interface ModelStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  refreshIntervalMs: number;
}

// Color palette for model bars — blue-purple spectrum
const MODEL_COLORS = [
  "#6366f1", // indigo
  "#8b5cf6", // violet
  "#a78bfa", // violet-400
  "#7c3aed", // violet-600
  "#4f46e5", // indigo-600
  "#818cf8", // indigo-400
  "#c4b5fd", // violet-300
  "#6d28d9", // violet-700
  "#4338ca", // indigo-700
  "#a5b4fc", // indigo-300
];

function formatTokenCount(tokens: number): string {
  if (tokens >= 1_000_000) {
    return `${(tokens / 1_000_000).toFixed(1)}M`;
  }
  if (tokens >= 1_000) {
    return `${(tokens / 1_000).toFixed(1)}k`;
  }
  return tokens.toLocaleString();
}

function ModelRow({
  model,
  index,
  maxTokens,
  totalTokensAll,
}: {
  model: ModelStats;
  index: number;
  maxTokens: number;
  totalTokensAll: number;
}) {
  const color = MODEL_COLORS[index % MODEL_COLORS.length];
  const percentage =
    totalTokensAll > 0 ? (model.totalTokens / totalTokensAll) * 100 : 0;
  const barWidth = maxTokens > 0 ? (model.totalTokens / maxTokens) * 100 : 0;

  return (
    <div className="group flex items-center gap-4 py-3 px-1 transition-colors hover:bg-white/20 dark:hover:bg-white/[0.03] rounded-xl">
      {/* Color dot */}
      <div className="flex-shrink-0 w-1 self-stretch rounded-full" style={{ backgroundColor: color }} />

      {/* Model name */}
      <div className="flex-1 min-w-0">
        <span className="text-sm font-medium text-foreground truncate block">
          {model.model}
        </span>
      </div>

      {/* Token stats */}
      <div className="flex items-center gap-6 flex-shrink-0">
        <div className="text-right min-w-[120px]">
          <span className="text-xs font-mono text-muted-foreground">
            <span className="text-foreground/80">{formatTokenCount(model.totalTokens)}</span>
            {" "}total
          </span>
        </div>

        {/* Percentage bar */}
        <div className="flex items-center gap-3 w-[160px] flex-shrink-0">
          <div className="flex-1 h-2 rounded-full bg-muted/40 overflow-hidden">
            <div
              className="h-full rounded-full transition-all duration-500 ease-out"
              style={{
                width: `${Math.max(barWidth, 2)}%`,
                background: `linear-gradient(90deg, ${color}dd, ${color})`,
              }}
            />
          </div>
          <span className="text-xs font-mono text-muted-foreground w-12 text-right">
            {percentage.toFixed(1)}%
          </span>
        </div>
      </div>
    </div>
  );
}

export function ModelStatsTable({
  range,
  appType,
  refreshIntervalMs,
}: ModelStatsTableProps) {
  const { t } = useTranslation();
  const { data: stats, isLoading } = useModelStats(range, appType, {
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
  });

  const { sorted, maxTokens, totalTokensAll } = useMemo(() => {
    if (!stats?.length) return { sorted: [], maxTokens: 0, totalTokensAll: 0 };

    const sorted = [...stats].sort((a, b) => b.totalTokens - a.totalTokens);
    const maxTokens = sorted[0]?.totalTokens ?? 0;
    const totalTokensAll = sorted.reduce((sum, m) => sum + m.totalTokens, 0);

    return { sorted, maxTokens, totalTokensAll };
  }, [stats]);

  if (isLoading) {
    return (
      <div className="flex h-[200px] items-center justify-center liquid-glass rounded-2xl">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground/30" />
      </div>
    );
  }

  if (!sorted.length) {
    return (
      <div className="liquid-glass rounded-2xl p-8 text-center text-sm text-muted-foreground">
        {t("usage.noData", "No data")}
      </div>
    );
  }

  return (
    <div className="liquid-glass rounded-2xl p-5">
      <div className="flex items-center justify-between mb-4">
        <h3 className="text-sm font-semibold text-foreground">
          {t("usage.modelBreakdown", "Model Breakdown")}
        </h3>
        <span className="text-xs text-muted-foreground">
          {sorted.length} {t("usage.models", "models")}
        </span>
      </div>

      {/* Header row */}
      <div className="flex items-center gap-4 py-2 px-1 text-xs text-muted-foreground border-b border-white/10 dark:border-white/5 mb-1">
        <div className="w-1 flex-shrink-0" />
        <div className="flex-1">{t("usage.model", "Model")}</div>
        <div className="min-w-[120px] text-right">{t("usage.tokens", "Tokens")}</div>
        <div className="w-[160px] flex-shrink-0 text-right">{t("usage.percentage", "Share")}</div>
      </div>

      {/* Model rows */}
      <div className="space-y-0.5">
        {sorted.map((model, index) => (
          <ModelRow
            key={model.model}
            model={model}
            index={index}
            maxTokens={maxTokens}
            totalTokensAll={totalTokensAll}
          />
        ))}
      </div>

      {/* Footer with totals */}
      <div className="flex items-center gap-4 py-3 px-1 mt-2 border-t border-white/10 dark:border-white/5">
        <div className="w-1 flex-shrink-0" />
        <div className="flex-1 text-sm font-medium text-muted-foreground">
          {t("usage.total", "Total")}
        </div>
        <div className="min-w-[120px] text-right">
          <span className="text-sm font-mono font-semibold text-foreground">
            {formatTokenCount(totalTokensAll)}
          </span>
        </div>
        <div className="w-[160px] flex-shrink-0 text-right">
          <span className="text-sm font-mono font-semibold text-foreground">
            100%
          </span>
        </div>
      </div>
    </div>
  );
}
