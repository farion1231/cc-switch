import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useProviderStats } from "@/lib/query/usage";
import { TablePagination, useClientPagination } from "./TablePagination";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  fmtUsd,
  formatTokensCompact,
  getLocaleFromLanguage,
  getResolvedLang,
} from "./format";
import { usageTable } from "./usageTable";
import { getUsageProviderLabel, usageProviderTitle } from "./providerLabel";
import { SuccessSpeedCells, SuccessSpeedHeaders } from "./statsColumns";
import {
  getColumnLayout,
  PROVIDER_STATS_COLUMNS,
  type ProviderStatsColumnId,
} from "./tableColumns";
import type { ColumnVisibility } from "@/types/table";
import type { UsageRangeSelection } from "@/types/usage";

interface ProviderStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
  columnVisibility?: ColumnVisibility<ProviderStatsColumnId>;
}

export function ProviderStatsTable({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
  columnVisibility,
}: ProviderStatsTableProps) {
  const { t, i18n } = useTranslation();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const columns = getColumnLayout(PROVIDER_STATS_COLUMNS, columnVisibility);
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
        <table
          className={usageTable.table}
          style={{ minWidth: columns.minWidth }}
          aria-label={t("usage.providerStats")}
        >
          <thead>
            <tr className={usageTable.headRow}>
              <th className={usageTable.th}>{t("usage.provider")}</th>
              {columns.isVisible("requests") && (
                <th className={usageTable.thEnd}>{t("usage.requests")}</th>
              )}
              {columns.isVisible("tokens") && (
                <th className={usageTable.thEnd}>{t("usage.tokens")}</th>
              )}
              {columns.isVisible("cost") && (
                <th className={usageTable.thEnd}>{t("usage.cost")}</th>
              )}
              <SuccessSpeedHeaders
                showSuccessRate={columns.isVisible("successRate")}
                showSpeed={columns.isVisible("speed")}
              />
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 ? (
              <tr>
                <td colSpan={columns.visibleCount} className={usageTable.empty}>
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
                      <span
                        className="block max-w-[260px] truncate"
                        title={usageProviderTitle(provider)}
                      >
                        {provider.label}
                      </span>
                    </td>
                    {columns.isVisible("requests") && (
                      <td className={usageTable.tdEnd}>
                        {fmtInt(stat.requestCount, locale)}
                      </td>
                    )}
                    {columns.isVisible("tokens") && (
                      <td
                        className={usageTable.tdEnd}
                        title={fmtInt(stat.totalTokens, locale)}
                      >
                        {formatTokensCompact(stat.totalTokens, locale)}
                      </td>
                    )}
                    {columns.isVisible("cost") && (
                      <td
                        className={cn(usageTable.tdEnd, "font-medium")}
                        title={fmtUsd(stat.totalCost, 6)}
                      >
                        {fmtUsd(stat.totalCost, 2)}
                      </td>
                    )}
                    <SuccessSpeedCells
                      stat={stat}
                      showSuccessRate={columns.isVisible("successRate")}
                      showSpeed={columns.isVisible("speed")}
                    />
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
