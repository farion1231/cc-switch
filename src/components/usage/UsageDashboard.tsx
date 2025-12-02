import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Card } from "@/components/ui/card";
import { UsageSummaryCards } from "./UsageSummaryCards";
import { UsageTrendChart } from "./UsageTrendChart";
import { RequestLogTable } from "./RequestLogTable";
import { ProviderStatsTable } from "./ProviderStatsTable";
import { ModelStatsTable } from "./ModelStatsTable";
import type { TimeRange } from "@/types/usage";

export function UsageDashboard() {
  const { t } = useTranslation();
  const [timeRange, setTimeRange] = useState<TimeRange>("1d");

  const days = timeRange === "1d" ? 1 : timeRange === "7d" ? 7 : 30;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-end">
        <select
          value={timeRange}
          onChange={(e) => setTimeRange(e.target.value as TimeRange)}
          className="rounded-md border px-3 py-1.5 text-sm"
        >
          <option value="1d">{t("usage.today", "今天")}</option>
          <option value="7d">{t("usage.last7days", "过去 7 天")}</option>
          <option value="30d">{t("usage.last30days", "过去 30 天")}</option>
        </select>
      </div>

      <UsageSummaryCards days={days} />

      <Card className="border-none bg-transparent p-0 shadow-none">
        <UsageTrendChart days={days} />
      </Card>

      <Tabs defaultValue="logs" className="w-full">
        <TabsList>
          <TabsTrigger value="logs">
            {t("usage.requestLogs", "请求日志")}
          </TabsTrigger>
          <TabsTrigger value="providers">
            {t("usage.providerStats", "Provider 统计")}
          </TabsTrigger>
          <TabsTrigger value="models">
            {t("usage.modelStats", "模型统计")}
          </TabsTrigger>
        </TabsList>

        <TabsContent value="logs" className="mt-4">
          <RequestLogTable />
        </TabsContent>

        <TabsContent value="providers" className="mt-4">
          <ProviderStatsTable />
        </TabsContent>

        <TabsContent value="models" className="mt-4">
          <ModelStatsTable />
        </TabsContent>
      </Tabs>
    </div>
  );
}
