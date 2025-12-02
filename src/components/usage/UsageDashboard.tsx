import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Card } from '@/components/ui/card';
import { UsageSummaryCards } from './UsageSummaryCards';
import { UsageTrendChart } from './UsageTrendChart';
import { RequestLogTable } from './RequestLogTable';
import { ProviderStatsTable } from './ProviderStatsTable';
import { ModelStatsTable } from './ModelStatsTable';
import type { TimeRange } from '@/types/usage';

export function UsageDashboard() {
  const { t } = useTranslation();
  const [timeRange, setTimeRange] = useState<TimeRange>('7d');

  const days = timeRange === '7d' ? 7 : timeRange === '30d' ? 30 : 90;

  return (
    <div className="space-y-6 p-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">{t('usage.title', '使用统计')}</h1>
        <select
          value={timeRange}
          onChange={(e) => setTimeRange(e.target.value as TimeRange)}
          className="rounded-md border px-3 py-1.5"
        >
          <option value="7d">{t('usage.last7days', '最近 7 天')}</option>
          <option value="30d">{t('usage.last30days', '最近 30 天')}</option>
          <option value="90d">{t('usage.last90days', '最近 90 天')}</option>
        </select>
      </div>

      <UsageSummaryCards days={days} />

      <Card className="p-6">
        <UsageTrendChart days={days} />
      </Card>

      <Tabs defaultValue="logs" className="w-full">
        <TabsList>
          <TabsTrigger value="logs">{t('usage.requestLogs', '请求日志')}</TabsTrigger>
          <TabsTrigger value="providers">{t('usage.providerStats', 'Provider 统计')}</TabsTrigger>
          <TabsTrigger value="models">{t('usage.modelStats', '模型统计')}</TabsTrigger>
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
