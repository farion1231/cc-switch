import { useState, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  BarChart2,
  DollarSign,
  Activity,
  TrendingUp,
  ChevronRight,
  Settings,
  RefreshCw,
  Layers,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { usageApi } from "@/lib/api/usage";
import type { TimeRange } from "@/types/usage";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";

interface UsageSidebarWidgetProps {
  isExpanded?: boolean;
  onToggleExpand?: (expanded: boolean) => void;
  onOpenSettings?: () => void;
}

const getWindow = (days: number) => {
  const endDate = Math.floor(Date.now() / 1000);
  const startDate = endDate - days * 24 * 60 * 60;
  return { startDate, endDate };
};

export function UsageSidebarWidget({
  isExpanded: controlledExpanded,
  onToggleExpand,
  onOpenSettings,
}: UsageSidebarWidgetProps) {
  const { t } = useTranslation();
  const [internalExpanded, setInternalExpanded] = useState(false);
  const [timeRange, setTimeRange] = useState<TimeRange>("1d");
  const [shouldFetch, setShouldFetch] = useState(false);
  const queryClient = useQueryClient();
  const isExpanded = controlledExpanded ?? internalExpanded;
  const setExpanded = (val: boolean) => {
    if (controlledExpanded === undefined) {
      setInternalExpanded(val);
    }
    onToggleExpand?.(val);
  };

  const days = timeRange === "1d" ? 1 : timeRange === "7d" ? 7 : 30;
  const {
    data: summary,
    isLoading,
    refetch,
  } = useQuery({
    queryKey: ["usage", "summary", days],
    queryFn: async () => {
      const { startDate, endDate } = getWindow(days);
      return usageApi.getUsageSummary(startDate, endDate);
    },
    enabled: shouldFetch,
  });

  useEffect(() => {
    if (isExpanded && !shouldFetch) {
      setShouldFetch(true);
      void refetch();
    }
  }, [isExpanded, shouldFetch, refetch]);

  const totalCost = parseFloat(summary?.totalCost || "0");
  const totalRequests = summary?.totalRequests ?? 0;
  const totalTokens =
    (summary?.totalInputTokens ?? 0) + (summary?.totalOutputTokens ?? 0);
  const successRate = summary?.successRate ?? 100;

  const handleRefresh = () => {
    void queryClient.invalidateQueries({ queryKey: ["usage", "summary"] });
    void refetch();
  };

  return (
    <div
      className={cn(
        "fixed right-0 top-1/2 -translate-y-1/2 z-40 flex items-center transition-all duration-300 ease-in-out",
        isExpanded ? "translate-x-0" : "translate-x-[calc(100%-48px)]",
      )}
    >
      <AnimatePresence mode="wait">
        {isExpanded && (
          <motion.div
            initial={{ opacity: 0, x: 20 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: 20 }}
            transition={{ duration: 0.2 }}
            className="mr-2"
          >
            <Card className="w-72 glass-card border-border/50 bg-background/80 backdrop-blur-xl shadow-xl rounded-xl overflow-hidden">
              <CardContent className="p-4 space-y-4">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <BarChart2 className="h-5 w-5 text-primary" />
                    <h3 className="font-semibold">{t("usage.title")}</h3>
                  </div>
                  <div className="flex items-center gap-1">
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={handleRefresh}
                      className="h-7 w-7 hover:bg-muted/50"
                      title={t("common.refresh")}
                    >
                      <RefreshCw
                        className={cn(
                          "h-4 w-4",
                          isLoading ? "animate-spin" : "",
                        )}
                      />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={onOpenSettings}
                      className="h-7 w-7 hover:bg-muted/50"
                      title={t("common.settings")}
                    >
                      <Settings className="h-4 w-4" />
                    </Button>
                  </div>
                </div>

                <div className="flex gap-1 p-1 bg-muted/50 rounded-lg">
                  {(["1d", "7d", "30d"] as TimeRange[]).map((range) => (
                    <button
                      key={range}
                      onClick={() => setTimeRange(range)}
                      className={cn(
                        "flex-1 px-2 py-1.5 text-xs font-medium rounded-md transition-all duration-200",
                        timeRange === range
                          ? "bg-primary/10 text-primary shadow-sm"
                          : "text-muted-foreground hover:text-foreground hover:bg-muted",
                      )}
                    >
                      {range === "1d"
                        ? t("usage.today")
                        : range === "7d"
                          ? t("usage.last7days")
                          : t("usage.last30days")}
                    </button>
                  ))}
                </div>

                <div className="grid grid-cols-2 gap-3">
                  <div className="p-3 rounded-lg bg-green-500/5 border border-green-500/10">
                    <div className="flex items-center gap-1.5 mb-1">
                      <DollarSign className="h-3.5 w-3.5 text-green-500" />
                      <span className="text-xs text-muted-foreground">
                        {t("usage.totalCost")}
                      </span>
                    </div>
                    <p className="text-lg font-bold truncate">
                      ${totalCost.toFixed(4)}
                    </p>
                  </div>

                  <div className="p-3 rounded-lg bg-blue-500/5 border border-blue-500/10">
                    <div className="flex items-center gap-1.5 mb-1">
                      <Activity className="h-3.5 w-3.5 text-blue-500" />
                      <span className="text-xs text-muted-foreground">
                        {t("usage.totalRequests")}
                      </span>
                    </div>
                    <p className="text-lg font-bold truncate">
                      {totalRequests.toLocaleString()}
                    </p>
                  </div>

                  <div className="p-3 rounded-lg bg-purple-500/5 border border-purple-500/10">
                    <div className="flex items-center gap-1.5 mb-1">
                      <Layers className="h-3.5 w-3.5 text-purple-500" />
                      <span className="text-xs text-muted-foreground">
                        {t("usage.totalTokens")}
                      </span>
                    </div>
                    <p className="text-lg font-bold truncate">
                      {(totalTokens / 1000).toFixed(1)}k
                    </p>
                  </div>

                  <div className="p-3 rounded-lg bg-orange-500/5 border border-orange-500/10">
                    <div className="flex items-center gap-1.5 mb-1">
                      <TrendingUp className="h-3.5 w-3.5 text-orange-500" />
                      <span className="text-xs text-muted-foreground">
                        {t("usage.successRate")}
                      </span>
                    </div>
                    <p className="text-lg font-bold truncate">
                      {successRate.toFixed(1)}%
                    </p>
                  </div>
                </div>

                <div className="pt-2 border-t border-border/50">
                  <div className="flex items-center justify-between text-xs text-muted-foreground">
                    <span>
                      {t("usage.lastUpdated", {
                        defaultValue: "Last updated",
                      })}
                    </span>
                    <span>
                      {isLoading ? "..." : new Date().toLocaleTimeString()}
                    </span>
                  </div>
                </div>
              </CardContent>
            </Card>
          </motion.div>
        )}
      </AnimatePresence>

      <Button
        variant="secondary"
        size="icon"
        onClick={() => setExpanded(!isExpanded)}
        className={cn(
          "h-16 w-12 rounded-l-lg rounded-r-none glass shadow-lg border-y border-l border-border/50 bg-background/80 backdrop-blur-xl hover:bg-muted/80 transition-all",
          isExpanded && "bg-muted/80",
        )}
      >
        {isExpanded ? (
          <ChevronRight className="h-5 w-5" />
        ) : (
          <div className="flex flex-col items-center gap-1">
            <BarChart2 className="h-4 w-4" />
            <DollarSign className="h-3 w-3" />
          </div>
        )}
      </Button>
    </div>
  );
}
