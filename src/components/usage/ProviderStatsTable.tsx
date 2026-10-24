import { Fragment, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDown, ChevronDown, ChevronRight } from "lucide-react";
import { useProviderStats } from "@/lib/query/usage";
import { TablePagination, useClientPagination } from "./TablePagination";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  formatTokensCompact,
  getLocaleFromLanguage,
  getResolvedLang,
} from "./format";
import { usageTable } from "./usageTable";
import { getUsageProviderLabel, usageProviderTitle } from "./providerLabel";
import { PerformanceCells, PerformanceHeaders } from "./statsColumns";
import { ProviderUsageBreakdown } from "./ProviderUsageBreakdown";
import type { UsageRangeSelection } from "@/types/usage";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";

interface ProviderStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  providerAppType?: string;
  providerId?: string;
  providerName?: string;
  projectKey?: string;
  model?: string;
  refreshIntervalMs: number;
}

export function ProviderStatsTable({
  range,
  appType,
  providerAppType,
  providerId,
  providerName,
  projectKey,
  model,
  refreshIntervalMs,
}: ProviderStatsTableProps) {
  const { t, i18n } = useTranslation();
  const { money, moneyTitle } = useMoneyFormat();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const { data: stats, isLoading } = useProviderStats(
    range,
    { appType, providerAppType, providerId, providerName, projectKey, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  // 画板：按请求数排序（后端按成本排）
  const rows = useMemo(
    () => [...(stats ?? [])].sort((a, b) => b.requestCount - a.requestCount),
    [stats],
  );
  const pagination = useClientPagination(
    rows,
    JSON.stringify([
      range,
      appType,
      providerAppType,
      providerId,
      providerName,
      projectKey,
      model,
    ]),
  );
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  const toggle = (key: string) => {
    setExpanded((previous) => {
      const next = new Set(previous);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  };

  if (isLoading) {
    return <div className={usageTable.skeleton} />;
  }

  return (
    <div className="flex flex-col">
      <div className={usageTable.scroller}>
        {/* 和请求日志同一套列组与取宽：按内容取宽、至少铺满面板，放不下横向滚动 */}
        <table
          className={cn(usageTable.table, "w-max min-w-full")}
          aria-label={t("usage.providerStats")}
        >
          <thead>
            <tr className={usageTable.headRow}>
              <th className={usageTable.th}>{t("usage.provider")}</th>
              {/* 按请求数从多到少排：箭头就是排序说明，不另写文字 */}
              <th className={usageTable.th} aria-sort="descending">
                <span className="inline-flex items-center gap-1">
                  {t("usage.requests")}
                  <ArrowDown
                    aria-hidden="true"
                    className="h-3 w-3 text-fg-3"
                    strokeWidth={2}
                  />
                </span>
              </th>
              <th className={cn(usageTable.th, usageTable.groupGap)}>
                {t("usage.cost")}
              </th>
              <th className={cn(usageTable.th, usageTable.groupGap)}>
                {t("usage.tokens")}
              </th>
              <th className={usageTable.th}>{t("usage.cacheHitRate")}</th>
              <PerformanceHeaders />
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 ? (
              <tr>
                <td colSpan={8} className={usageTable.empty}>
                  {t("usage.noData")}
                </td>
              </tr>
            ) : (
              pagination.pageRows.map((stat) => {
                const provider = getUsageProviderLabel(stat.providerName, t);
                const rowKey = JSON.stringify([stat.appType, stat.providerId]);
                const isExpanded = expanded.has(rowKey);
                return (
                  <Fragment key={rowKey}>
                    <tr className={usageTable.row}>
                      <td className={usageTable.td}>
                        <span className="inline-flex max-w-[260px] items-center gap-1">
                          <button
                            type="button"
                            className="grid size-5 shrink-0 place-items-center rounded-[4px] text-fg-2 hover:bg-subtle focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                            aria-label={t("usage.providers.expandModels")}
                            aria-expanded={isExpanded}
                            onClick={() => toggle(rowKey)}
                          >
                            {isExpanded ? (
                              <ChevronDown className="size-3.5" />
                            ) : (
                              <ChevronRight className="size-3.5" />
                            )}
                          </button>
                          <span
                            className="truncate"
                            title={usageProviderTitle(provider)}
                          >
                            {provider.label}
                          </span>
                        </span>
                      </td>
                      <td className={usageTable.td}>
                        {fmtInt(stat.requestCount, locale)}
                      </td>
                      <td
                        className={cn(
                          usageTable.td,
                          usageTable.groupGap,
                          "font-medium",
                        )}
                        title={moneyTitle(stat.totalCost, 6)}
                      >
                        {money(stat.totalCost, 2)}
                      </td>
                      <td
                        className={cn(usageTable.td, usageTable.groupGap)}
                        title={fmtInt(stat.totalTokens, locale)}
                      >
                        {formatTokensCompact(stat.totalTokens, locale)}
                      </td>
                      <td className={usageTable.td}>
                        {(stat.cacheableInputTokens ?? 0) > 0
                          ? `${((stat.cacheHitRate ?? 0) * 100).toFixed(1)}%`
                          : "—"}
                      </td>
                      <PerformanceCells stat={stat} />
                    </tr>
                    {isExpanded ? (
                      <ProviderUsageBreakdown
                        providerId={stat.providerId}
                        providerAppType={stat.appType}
                        providerName={stat.providerName}
                        projectKey={projectKey}
                        model={model}
                        range={range}
                        refreshIntervalMs={refreshIntervalMs}
                        colSpan={8}
                      />
                    ) : null}
                  </Fragment>
                );
              })
            )}
          </tbody>
        </table>
      </div>
      <TablePagination
        page={pagination.page}
        totalPages={pagination.totalPages}
        total={pagination.total}
        onPageChange={pagination.setPage}
      />
    </div>
  );
}
