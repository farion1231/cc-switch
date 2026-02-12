import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { UsageSummaryCards } from "./UsageSummaryCards";
import { UsageTrendChart } from "./UsageTrendChart";
import { RequestLogTable } from "./RequestLogTable";
import { ProviderStatsTable } from "./ProviderStatsTable";
import { ModelStatsTable } from "./ModelStatsTable";
import type { TimeRange } from "@/types/usage";
import { motion } from "framer-motion";
import { BarChart3, ListFilter, Activity, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useQueryClient } from "@tanstack/react-query";
import { usageKeys } from "@/lib/query/usage";

export function UsageDashboard() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [timeRange, setTimeRange] = useState<TimeRange>("1d");
  const [refreshIntervalMs, setRefreshIntervalMs] = useState(30000);

  const refreshIntervalOptionsMs = [0, 5000, 10000, 30000, 60000] as const;
  const changeRefreshInterval = () => {
    const currentIndex = refreshIntervalOptionsMs.indexOf(
      refreshIntervalMs as (typeof refreshIntervalOptionsMs)[number],
    );
    const safeIndex = currentIndex >= 0 ? currentIndex : 3; // default 30s
    const nextIndex = (safeIndex + 1) % refreshIntervalOptionsMs.length;
    const next = refreshIntervalOptionsMs[nextIndex];
    setRefreshIntervalMs(next);
    queryClient.invalidateQueries({ queryKey: usageKeys.all });
  };

  const days = timeRange === "1d" ? 1 : timeRange === "7d" ? 7 : 30;

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className="space-y-8 pb-8"
    >
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div className="flex flex-col gap-1">
          <h2 className="text-2xl font-bold">{t("usage.title")}</h2>
          <p className="text-sm text-muted-foreground">{t("usage.subtitle")}</p>
        </div>

        <Tabs
          value={timeRange}
          onValueChange={(v) => setTimeRange(v as TimeRange)}
          className="w-full sm:w-auto"
        >
          <div className="flex w-full sm:w-auto items-center gap-1">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-10 px-2 text-xs text-muted-foreground"
              title={t("common.refresh", "刷新")}
              onClick={changeRefreshInterval}
            >
              <RefreshCw className="mr-1 h-3.5 w-3.5" />
              {refreshIntervalMs > 0 ? `${refreshIntervalMs / 1000}s` : "--"}
            </Button>
            <TabsList className="flex w-full sm:w-auto bg-card/60 border border-border/50 backdrop-blur-sm shadow-sm h-10 p-1">
              <TabsTrigger
                value="1d"
                className="flex-1 sm:flex-none sm:px-6 data-[state=active]:bg-primary/10 data-[state=active]:text-primary hover:text-primary transition-colors"
              >
                {t("usage.today")}
              </TabsTrigger>
              <TabsTrigger
                value="7d"
                className="flex-1 sm:flex-none sm:px-6 data-[state=active]:bg-primary/10 data-[state=active]:text-primary hover:text-primary transition-colors"
              >
                {t("usage.last7days")}
              </TabsTrigger>
              <TabsTrigger
                value="30d"
                className="flex-1 sm:flex-none sm:px-6 data-[state=active]:bg-primary/10 data-[state=active]:text-primary hover:text-primary transition-colors"
              >
                {t("usage.last30days")}
              </TabsTrigger>
            </TabsList>
          </div>
        </Tabs>
      </div>

      <UsageSummaryCards days={days} refreshIntervalMs={refreshIntervalMs} />

      <UsageTrendChart days={days} refreshIntervalMs={refreshIntervalMs} />

      <div className="space-y-4">
        <Tabs defaultValue="logs" className="w-full">
          <div className="flex items-center justify-between mb-4">
            <TabsList className="bg-muted/50">
              <TabsTrigger value="logs" className="gap-2">
                <ListFilter className="h-4 w-4" />
                {t("usage.requestLogs")}
              </TabsTrigger>
              <TabsTrigger value="providers" className="gap-2">
                <Activity className="h-4 w-4" />
                {t("usage.providerStats")}
              </TabsTrigger>
              <TabsTrigger value="models" className="gap-2">
                <BarChart3 className="h-4 w-4" />
                {t("usage.modelStats")}
              </TabsTrigger>
            </TabsList>
          </div>

          <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.2 }}
          >
            <TabsContent value="logs" className="mt-0">
              <RequestLogTable refreshIntervalMs={refreshIntervalMs} />
            </TabsContent>

            <TabsContent value="providers" className="mt-0">
              <ProviderStatsTable refreshIntervalMs={refreshIntervalMs} />
            </TabsContent>

            <TabsContent value="models" className="mt-0">
              <ModelStatsTable refreshIntervalMs={refreshIntervalMs} />
            </TabsContent>
          </motion.div>
        </Tabs>
      </div>
    </motion.div>
  );
}
