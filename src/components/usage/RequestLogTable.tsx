import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { useRequestLogs } from '@/lib/query/usage';
import type { LogFilters } from '@/types/usage';

export function RequestLogTable() {
  const { t } = useTranslation();
  const [filters] = useState<LogFilters>({});
  const [page, setPage] = useState(0);
  const limit = 20;

  const { data: logs, isLoading } = useRequestLogs(filters, limit, page * limit);

  if (isLoading) {
    return <div className="h-[400px] animate-pulse rounded bg-gray-100" />;
  }

  return (
    <div className="space-y-4">
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t('usage.time', '时间')}</TableHead>
              <TableHead>{t('usage.provider', 'Provider')}</TableHead>
              <TableHead>{t('usage.model', '模型')}</TableHead>
              <TableHead className="text-right">{t('usage.tokens', 'Tokens')}</TableHead>
              <TableHead className="text-right">{t('usage.cost', '成本')}</TableHead>
              <TableHead className="text-right">{t('usage.latency', '延迟')}</TableHead>
              <TableHead>{t('usage.status', '状态')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {logs?.length === 0 ? (
              <TableRow>
                <TableCell colSpan={7} className="text-center text-muted-foreground">
                  {t('usage.noData', '暂无数据')}
                </TableCell>
              </TableRow>
            ) : (
              logs?.map((log) => (
                <TableRow key={log.requestId}>
                  <TableCell>
                    {new Date(log.createdAt * 1000).toLocaleString('zh-CN')}
                  </TableCell>
                  <TableCell className="font-mono text-sm">{log.providerId}</TableCell>
                  <TableCell className="font-mono text-sm">{log.model}</TableCell>
                  <TableCell className="text-right">
                    {(log.inputTokens + log.outputTokens).toLocaleString()}
                  </TableCell>
                  <TableCell className="text-right">
                    ${parseFloat(log.totalCostUsd).toFixed(6)}
                  </TableCell>
                  <TableCell className="text-right">{log.latencyMs}ms</TableCell>
                  <TableCell>
                    <span
                      className={`inline-flex rounded-full px-2 py-1 text-xs ${
                        log.statusCode >= 200 && log.statusCode < 300
                          ? 'bg-green-100 text-green-800'
                          : 'bg-red-100 text-red-800'
                      }`}
                    >
                      {log.statusCode}
                    </span>
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </div>

      {logs && logs.length >= limit && (
        <div className="flex justify-center gap-2">
          <button
            onClick={() => setPage(Math.max(0, page - 1))}
            disabled={page === 0}
            className="rounded border px-3 py-1 disabled:opacity-50"
          >
            {t('common.previous', '上一页')}
          </button>
          <span className="px-3 py-1">
            {t('common.page', '第')} {page + 1} {t('common.pageUnit', '页')}
          </span>
          <button
            onClick={() => setPage(page + 1)}
            disabled={logs.length < limit}
            className="rounded border px-3 py-1 disabled:opacity-50"
          >
            {t('common.next', '下一页')}
          </button>
        </div>
      )}
    </div>
  );
}
