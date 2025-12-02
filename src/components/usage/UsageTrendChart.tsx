import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts';
import { useUsageTrends } from '@/lib/query/usage';

interface UsageTrendChartProps {
  days: number;
}

type MetricType = 'requests' | 'cost' | 'tokens';

export function UsageTrendChart({ days }: UsageTrendChartProps) {
  const { t } = useTranslation();
  const [metric, setMetric] = useState<MetricType>('requests');
  const { data: trends, isLoading } = useUsageTrends(days);

  if (isLoading) {
    return <div className="h-[300px] animate-pulse rounded bg-gray-100" />;
  }

  const chartData = trends?.map((stat) => ({
    date: new Date(stat.date).toLocaleDateString('zh-CN', { month: '2-digit', day: '2-digit' }),
    requests: stat.requestCount,
    cost: parseFloat(stat.totalCost),
    tokens: stat.totalTokens,
  })) || [];

  const getYAxisLabel = () => {
    switch (metric) {
      case 'requests': return t('usage.requests', '请求数');
      case 'cost': return t('usage.cost', '成本 (USD)');
      case 'tokens': return t('usage.tokens', 'Tokens');
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-semibold">{t('usage.trends', '使用趋势')}</h3>
        <div className="flex gap-2">
          <button
            onClick={() => setMetric('requests')}
            className={`rounded px-3 py-1 text-sm ${
              metric === 'requests' ? 'bg-primary text-primary-foreground' : 'bg-secondary'
            }`}
          >
            {t('usage.requests', '请求数')}
          </button>
          <button
            onClick={() => setMetric('cost')}
            className={`rounded px-3 py-1 text-sm ${
              metric === 'cost' ? 'bg-primary text-primary-foreground' : 'bg-secondary'
            }`}
          >
            {t('usage.cost', '成本')}
          </button>
          <button
            onClick={() => setMetric('tokens')}
            className={`rounded px-3 py-1 text-sm ${
              metric === 'tokens' ? 'bg-primary text-primary-foreground' : 'bg-secondary'
            }`}
          >
            {t('usage.tokens', 'Tokens')}
          </button>
        </div>
      </div>

      <ResponsiveContainer width="100%" height={300}>
        <LineChart data={chartData}>
          <CartesianGrid strokeDasharray="3 3" />
          <XAxis dataKey="date" />
          <YAxis label={{ value: getYAxisLabel(), angle: -90, position: 'insideLeft' }} />
          <Tooltip />
          <Line type="monotone" dataKey={metric} stroke="#8884d8" strokeWidth={2} />
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
}
