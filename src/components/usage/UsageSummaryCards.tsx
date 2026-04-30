import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useUsageSummary } from "@/lib/query/usage";
import { Activity, DollarSign, Layers, Database, Loader2 } from "lucide-react";
import { motion } from "framer-motion";
import { fmtUsd, parseFiniteNumber } from "./format";
import type { UsageRangeSelection } from "@/types/usage";

interface UsageSummaryCardsProps {
  range: UsageRangeSelection;
  appType?: string;
  refreshIntervalMs: number;
}

export function UsageSummaryCards({
  range,
  appType,
  refreshIntervalMs,
}: UsageSummaryCardsProps) {
  const { t } = useTranslation();

  const { data: summary, isLoading } = useUsageSummary(range, appType, {
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
  });

  const stats = useMemo(() => {
    const totalRequests = summary?.totalRequests ?? 0;
    const totalCost = parseFiniteNumber(summary?.totalCost);

    const inputTokens = summary?.totalInputTokens ?? 0;
    const outputTokens = summary?.totalOutputTokens ?? 0;
    const totalTokens = inputTokens + outputTokens;

    const cacheWriteTokens = summary?.totalCacheCreationTokens ?? 0;
    const cacheReadTokens = summary?.totalCacheReadTokens ?? 0;
    const totalCacheTokens = cacheWriteTokens + cacheReadTokens;

    return [
      {
        title: t("usage.totalRequests"),
        value: totalRequests.toLocaleString(),
        icon: Activity,
        color: "text-blue-500",
        bg: "bg-blue-500/10",
        subValue: null,
      },
      {
        title: t("usage.totalCost"),
        value: totalCost == null ? "--" : fmtUsd(totalCost, 4),
        icon: DollarSign,
        color: "text-green-500",
        bg: "bg-green-500/10",
        subValue: null,
      },
      {
        title: t("usage.totalTokens"),
        value: totalTokens.toLocaleString(),
        icon: Layers,
        color: "text-purple-500",
        bg: "bg-purple-500/10",
        subValue: (
          <div className="flex flex-col gap-1 text-xs text-muted-foreground mt-3 pt-3 border-t border-border/50">
            <div className="flex justify-between items-center">
              <span>{t("usage.input")}</span>
              <span className="text-foreground/80">
                {(inputTokens / 1000).toFixed(1)}k
              </span>
            </div>
            <div className="flex justify-between items-center">
              <span>{t("usage.output")}</span>
              <span className="text-foreground/80">
                {(outputTokens / 1000).toFixed(1)}k
              </span>
            </div>
          </div>
        ),
      },
      {
        title: t("usage.cacheTokens"),
        value: totalCacheTokens.toLocaleString(),
        icon: Database,
        color: "text-orange-500",
        bg: "bg-orange-500/10",
        subValue: (
          <div className="flex flex-col gap-1 text-xs text-muted-foreground mt-3 pt-3 border-t border-border/50">
            <div className="flex justify-between items-center">
              <span>{t("usage.cacheWrite")}</span>
              <span className="text-foreground/80">
                {(cacheWriteTokens / 1000).toFixed(1)}k
              </span>
            </div>
            <div className="flex justify-between items-center">
              <span>{t("usage.cacheRead")}</span>
              <span className="text-foreground/80">
                {(cacheReadTokens / 1000).toFixed(1)}k
              </span>
            </div>
          </div>
        ),
      },
    ];
  }, [summary, t]);

  const container = {
    hidden: { opacity: 0 },
    show: {
      opacity: 1,
      transition: {
        staggerChildren: 0.05,
      },
    },
  };

  const item = {
    hidden: { opacity: 0, y: 8, scale: 0.98 },
    show: {
      opacity: 1,
      y: 0,
      scale: 1,
      transition: {
        type: "spring" as const,
        stiffness: 500,
        damping: 32,
        mass: 0.6,
      },
    },
  };

  if (isLoading) {
    return (
      <div className="grid gap-3 md:grid-cols-4">
        {[...Array(4)].map((_, i) => (
          <div
            key={i}
            className="liquid-glass rounded-2xl h-[140px] flex items-center justify-center"
          >
            <Loader2 className="h-5 w-5 animate-spin text-muted-foreground/30" />
          </div>
        ))}
      </div>
    );
  }

  return (
    <motion.div
      variants={container}
      initial="hidden"
      animate="show"
      className="grid gap-3 md:grid-cols-4"
    >
      {stats.map((stat, i) => (
        <motion.div key={i} variants={item}>
          <div className="liquid-glass rounded-2xl p-4 transition-all duration-200 hover:bg-white/50 dark:hover:bg-white/[0.06]">
            <div className="flex items-start justify-between mb-3">
              <p className="text-xs font-medium text-muted-foreground">
                {stat.title}
              </p>
              <div className={`p-1.5 rounded-lg ${stat.bg}`}>
                <stat.icon className={`h-3.5 w-3.5 ${stat.color}`} />
              </div>
            </div>

            <h3 className="text-xl font-bold truncate" title={stat.value}>
              {stat.value}
            </h3>

            {stat.subValue || <div className="mt-3 pt-3 border-t border-transparent h-[48px]" />}
          </div>
        </motion.div>
      ))}
    </motion.div>
  );
}
