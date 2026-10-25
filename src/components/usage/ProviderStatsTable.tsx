import { usageCostLabel } from "@/components/usage/format";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDown } from "lucide-react";
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
import type { UsageRangeSelection } from "@/types/usage";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";

interface ProviderStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

export function ProviderStatsTable({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: ProviderStatsTableProps) {
  const { t, i18n } = useTranslation();
  const { money, moneyTitle } = useMoneyFormat();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const { data: stats, isLoading } = useProviderStats(
    range,
    { appType, providerName, model },
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
    JSON.stringify([range, appType, providerName, model]),
  );

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
              <PerformanceHeaders />
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 ? (
              <tr>
                <td colSpan={7} className={usageTable.empty}>
                  {t("usage.noData")}
                </td>
              </tr>
            ) : (
              pagination.pageRows.map((stat) => {
                const provider = getUsageProviderLabel(stat.providerName, t);
                return (
                  <tr
                    key={JSON.stringify([stat.appType, stat.providerId])}
                    className={usageTable.row}
                  >
                    <td className={usageTable.td}>
                      <span title={usageProviderTitle(provider)}>
                        {provider.label}
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
                      title={
                        stat.costIncomplete
                          ? t("usage.costIncomplete")
                          : moneyTitle(stat.totalCost, 6)
                      }
                    >
                      {usageCostLabel(
                        stat.totalCost,
                        stat.costIncomplete,
                        money(stat.totalCost, 2),
                        t("usage.unpriced"),
                      )}
                    </td>
                    <td
                      className={cn(usageTable.td, usageTable.groupGap)}
                      title={fmtInt(stat.totalTokens, locale)}
                    >
                      {formatTokensCompact(stat.totalTokens, locale)}
                    </td>
                    <PerformanceCells stat={stat} />
                  </tr>
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
