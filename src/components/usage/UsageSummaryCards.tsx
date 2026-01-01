import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Card, CardContent } from "@/components/ui/card";
import { useUsageSummary } from "@/lib/query/usage";
import { Activity, DollarSign, Layers, Database, Loader2 } from "lucide-react";
import { motion } from "framer-motion";

interface UsageSummaryCardsProps {
  days: number;
}

export function UsageSummaryCards({ days }: UsageSummaryCardsProps) {
  const { t } = useTranslation();

  const { data: summary, isLoading } = useUsageSummary(days);

  const stats = useMemo(() => {
    const totalRequests = summary?.totalRequests ?? 0;
    const totalCost = parseFloat(summary?.totalCost || "0");

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
        value: `$${totalCost.toFixed(4)}`,
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

              <div className="space-y-1">
                <h3 className="text-2xl font-bold truncate" title={stat.value}>
                  {stat.value}
                </h3>
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
