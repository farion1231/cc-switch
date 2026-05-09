import { useMemo } from "react";
import type React from "react";
import { useTranslation } from "react-i18next";
import { Card, CardContent } from "@/components/ui/card";
import { useUsageSummary } from "@/lib/query/usage";
import { Activity, DollarSign, Layers, Database, Loader2 } from "lucide-react";
import { motion } from "framer-motion";
import { fmtUsd, fmtTokenAbbr, parseFiniteNumber } from "./format";
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
    const cacheWriteTokens = summary?.totalCacheCreationTokens ?? 0;
    const cacheReadTokens = summary?.totalCacheReadTokens ?? 0;
    const totalTokens = inputTokens + outputTokens + cacheWriteTokens + cacheReadTokens;
    const totalCacheTokens = cacheWriteTokens + cacheReadTokens;
    const totalInputWithCache = inputTokens + cacheWriteTokens + cacheReadTokens;

    const cacheHitRate =
      totalCacheTokens > 0
        ? (cacheReadTokens / totalCacheTokens) * 100
        : null;

    return [
      {
        title: t("usage.totalRequests"),
        value: totalRequests.toLocaleString(),
        abbr: undefined as string | undefined,
        icon: Activity,
        color: "text-blue-500",
        bg: "bg-blue-500/10",
        subValue: null as React.ReactNode,
      },
      {
        title: t("usage.totalCost"),
        value: totalCost == null ? "--" : fmtUsd(totalCost, 4),
        abbr: undefined as string | undefined,
        icon: DollarSign,
        color: "text-green-500",
        bg: "bg-green-500/10",
        subValue: null as React.ReactNode,
      },
      {
        title: t("usage.totalTokens"),
        value: totalTokens.toLocaleString(),
        abbr: fmtTokenAbbr(totalTokens),
        icon: Layers,
        color: "text-purple-500",
        bg: "bg-purple-500/10",
        subValue: (
          <div className="flex flex-col gap-1 text-xs text-muted-foreground mt-3 pt-3 border-t border-border/50">
            <div className="flex justify-between items-center">
              <span>{t("usage.input")}</span>
              <span className="text-foreground/80">
                {fmtTokenAbbr(totalInputWithCache)}
              </span>
            </div>
            <div className="flex justify-between items-center">
              <span>{t("usage.output")}</span>
              <span className="text-foreground/80">
                {fmtTokenAbbr(outputTokens)}
              </span>
            </div>
          </div>
        ) as React.ReactNode,
      },
      {
        title: t("usage.cacheTokens"),
        value: totalCacheTokens.toLocaleString(),
        abbr: undefined as string | undefined,
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
            {cacheHitRate != null && (
              <div className="flex justify-between items-center pt-1 border-t border-border/30">
                <span>{t("usage.cacheHitRate")}</span>
                <span className="text-foreground/80 font-medium">
                  {cacheHitRate.toFixed(1)}%
                </span>
              </div>
            )}
          </div>
        ) as React.ReactNode,
      },
    ];
  }, [summary, t]);

  const container = {
    hidden: { opacity: 0 },
    show: {
      opacity: 1,
      transition: {
        staggerChildren: 0.1,
      },
    },
  };

  const item = {
    hidden: { opacity: 0, y: 20 },
    show: { opacity: 1, y: 0 },
  };

  if (isLoading) {
    return (
      <div className="grid gap-4 md:grid-cols-4">
        {[...Array(4)].map((_, i) => (
          <Card
            key={i}
            className="border border-border/50 bg-card/40 backdrop-blur-sm shadow-sm"
          >
            <CardContent className="p-6 flex items-center justify-center min-h-[160px]">
              <Loader2 className="h-6 w-6 animate-spin text-muted-foreground/50" />
            </CardContent>
          </Card>
        ))}
      </div>
    );
  }

  return (
    <motion.div
      variants={container}
      initial="hidden"
      animate="show"
      className="grid gap-4 md:grid-cols-4"
    >
      {stats.map((stat, i) => (
        <motion.div key={i} variants={item}>
          <Card className="relative h-full overflow-hidden border border-border/50 bg-gradient-to-br from-card/50 to-background/50 backdrop-blur-xl hover:from-card/60 hover:to-background/60 transition-all shadow-sm">
            <CardContent className="p-5">
              <div className="flex items-start justify-between mb-2">
                <p className="text-sm font-medium text-muted-foreground">
                  {stat.title}
                </p>
                <div className={`p-2 rounded-lg ${stat.bg}`}>
                  <stat.icon className={`h-4 w-4 ${stat.color}`} />
                </div>
              </div>

              <div className="space-y-0.5">
                <h3 className="text-2xl font-bold truncate" title={stat.value}>
                  {stat.value}
                </h3>
                {stat.abbr && (
                  <div className={`text-4xl font-bold ${stat.color}`}>
                    {stat.abbr}
                  </div>
                )}
              </div>

              {stat.subValue || (
                /* Placeholder to properly align cards if no subvalue (first 2 cards) - effectively adding empty space or using flex-1 equivalent */
                <div className="mt-3 pt-3 border-t border-transparent h-[52px]"></div>
              )}
            </CardContent>
          </Card>
        </motion.div>
      ))}
    </motion.div>
  );
}
