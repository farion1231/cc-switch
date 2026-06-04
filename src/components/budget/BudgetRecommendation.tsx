import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import { Lightbulb, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { useQuery } from "@tanstack/react-query";
import { usageApi } from "@/lib/api/usage";
import type { BudgetFormData } from "@/lib/schemas/budget";

interface BudgetRecommendationProps {
  onApply: (rec: Recommendation) => void;
}

export interface Recommendation {
  name: string;
  scope: BudgetFormData["scope"];
  scopeValue?: string;
  period: BudgetFormData["period"];
  limitTokens?: number;
  limitUsd?: string;
}

interface RecRow {
  label: string;
  period: BudgetFormData["period"];
  tokens: number;
  usd: string;
  multiplier: number;
}

export function BudgetRecommendation({ onApply }: BudgetRecommendationProps) {
  const { t } = useTranslation();
  const [showDetails, setShowDetails] = useState(false);

  // 取最近 7 天数据推算
  const sevenDaysAgo = useMemo(() => {
    const d = new Date();
    d.setDate(d.getDate() - 7);
    return Math.floor(d.getTime() / 1000);
  }, []);

  const { data: summary, isLoading } = useQuery({
    queryKey: ["budget-recommendation", "7d"],
    queryFn: () => usageApi.getUsageSummary(sevenDaysAgo),
    refetchInterval: false,
    staleTime: 300000,
  });

  // 基于最近 7 天数据计算推荐
  const recommendations = useMemo((): {
    rows: RecRow[];
    dailyTokens: number;
    dailyUsd: number;
  } => {
    if (!summary) return { rows: [], dailyTokens: 0, dailyUsd: 0 };

    const dailyTokens = Math.ceil(summary.realTotalTokens / 7);
    const dailyUsd = parseFloat(summary.totalCost || "0") / 7;

    const rows: RecRow[] = [
      {
        label: t("budget.periodDaily"),
        period: "daily",
        tokens: dailyTokens,
        usd: dailyUsd.toFixed(2),
        multiplier: 1,
      },
      {
        label: t("budget.periodWeekly"),
        period: "weekly",
        tokens: dailyTokens * 7,
        usd: (dailyUsd * 7).toFixed(2),
        multiplier: 7,
      },
      {
        label: t("budget.periodMonthly"),
        period: "monthly",
        tokens: dailyTokens * 30,
        usd: (dailyUsd * 30).toFixed(2),
        multiplier: 30,
      },
    ];

    return { rows, dailyTokens, dailyUsd };
  }, [summary, t]);

  if (isLoading) {
    return (
      <div className="flex items-center gap-2 py-3 text-muted-foreground text-sm">
        <Loader2 className="h-4 w-4 animate-spin" />
        {t("budget.loadingRecommendation", {
          defaultValue: "正在分析历史消耗...",
        })}
      </div>
    );
  }

  if (!summary || summary.realTotalTokens === 0) {
    return null; // 没有历史数据不显示推荐
  }

  const { rows, dailyTokens, dailyUsd } = recommendations;

  return (
    <motion.div
      initial={{ opacity: 0, y: 5 }}
      animate={{ opacity: 1, y: 0 }}
      className="space-y-3"
    >
      <button
        type="button"
        onClick={() => setShowDetails(!showDetails)}
        className="flex items-center gap-2 text-sm font-medium text-amber-600 dark:text-amber-400 hover:opacity-80 transition-opacity"
      >
        <Lightbulb className="h-4 w-4" />
        {t("budget.recommendationTitle", {
          defaultValue: "推荐预算（基于近 7 天消耗）",
        })}
        <Badge variant="outline" className="text-xs font-normal">
          ~{formatTokens(dailyTokens)}/天 · ${dailyUsd.toFixed(2)}/天
        </Badge>
      </button>

      {showDetails && (
        <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
          {rows.map((row) => (
            <Card
              key={row.period}
              className="bg-card/40 backdrop-blur-sm border border-border/50"
            >
              <CardContent className="p-3 space-y-2">
                <p className="text-xs font-medium text-muted-foreground">
                  {row.label}
                </p>
                <div className="space-y-0.5">
                  <p className="text-sm">
                    <span className="text-muted-foreground">Tokens: </span>
                    <span className="font-semibold">
                      {formatTokens(row.tokens)}
                    </span>
                  </p>
                  <p className="text-sm">
                    <span className="text-muted-foreground">USD: </span>
                    <span className="font-semibold">${row.usd}</span>
                  </p>
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  className="w-full text-xs h-7"
                  onClick={() =>
                    onApply({
                      name: `${row.label}限额`,
                      scope: "global",
                      period: row.period,
                      limitTokens: row.tokens,
                      limitUsd: row.usd,
                    })
                  }
                >
                  {t("budget.applyRecommendation", { defaultValue: "应用" })}
                </Button>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </motion.div>
  );
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toLocaleString();
}
