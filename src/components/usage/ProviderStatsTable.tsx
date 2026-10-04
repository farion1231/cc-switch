import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useProviderStats } from "@/lib/query/usage";
import { TablePagination, useClientPagination } from "./TablePagination";
import { HelpTip } from "@/components/ui/help-tip";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  fmtUsd,
  formatTokensCompact,
  formatTokensPerSecond,
  getAggregateTokensPerSecond,
  getLocaleFromLanguage,
  getResolvedLang,
} from "./format";
import { usageTable } from "./usageTable";
import type { ProviderStats, UsageRangeSelection } from "@/types/usage";

interface ProviderStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

/** 一行供应商的汇总速度：Σ输出 ÷ Σ生成时间（后端只累加满足条件的请求）。 */
export function getProviderSpeed(stat: ProviderStats): string | null {
  return formatTokensPerSecond(
    getAggregateTokensPerSecond(stat.speedOutputTokens, stat.speedGenerationMs),
  );
}

/**
 * 会话日志导入的请求的汇总估算速度：Σ输出 ÷ Σ估算耗时（含首字等待）。
 * 没有精确速度时才拿它顶上，显示时前面带 ≈。
 */
export function getProviderEstimatedSpeed(stat: ProviderStats): string | null {
  return formatTokensPerSecond(
    getAggregateTokensPerSecond(
      stat.estSpeedOutputTokens,
      stat.estSpeedDurationMs,
    ),
  );
}

export function ProviderStatsTable({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: ProviderStatsTableProps) {
  const { t, i18n } = useTranslation();
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
        <table
          className={cn(usageTable.table, "min-w-[620px]")}
          aria-label={t("usage.providerStats")}
        >
          <thead>
            <tr className={usageTable.headRow}>
              <th className={usageTable.th}>{t("usage.provider")}</th>
              <th className={usageTable.thEnd}>{t("usage.requests")}</th>
              <th className={usageTable.thEnd}>{t("usage.tokens")}</th>
              <th className={usageTable.thEnd}>{t("usage.cost")}</th>
              <th className={usageTable.thEnd}>{t("usage.successRate")}</th>
              <th className={usageTable.thEnd}>
                <span className="inline-flex items-center gap-0.5">
                  {t("usage.speed")}
                  <HelpTip title={t("usage.speedSumHelpTitle")} align="end">
                    {t("usage.speedSumHelp")}
                  </HelpTip>
                </span>
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 ? (
              <tr>
                <td colSpan={6} className={usageTable.empty}>
                  {t("usage.noData")}
                </td>
              </tr>
            ) : (
              pagination.pageRows.map((stat) => {
                const exactSpeed = getProviderSpeed(stat);
                const estimatedSpeed =
                  exactSpeed == null ? getProviderEstimatedSpeed(stat) : null;
                const speed = exactSpeed ?? estimatedSpeed;
                return (
                  <tr
                    key={`${stat.providerId}:${stat.providerName}`}
                    className={usageTable.row}
                  >
                    <td className={usageTable.td}>
                      <span
                        className="block max-w-[260px] truncate"
                        title={stat.providerName}
                      >
                        {stat.providerName}
                      </span>
                    </td>
                    <td className={usageTable.tdEnd}>
                      {fmtInt(stat.requestCount, locale)}
                    </td>
                    <td
                      className={usageTable.tdEnd}
                      title={fmtInt(stat.totalTokens, locale)}
                    >
                      {formatTokensCompact(stat.totalTokens, locale)}
                    </td>
                    <td
                      className={cn(usageTable.tdEnd, "font-medium")}
                      title={fmtUsd(stat.totalCost, 6)}
                    >
                      {fmtUsd(stat.totalCost, 2)}
                    </td>
                    <td className={usageTable.tdEnd}>
                      {stat.successRate.toFixed(
                        stat.successRate >= 99.95 ? 0 : 1,
                      )}
                      %
                    </td>
                    <td
                      className={cn(
                        usageTable.tdEnd,
                        speed == null && usageTable.muted,
                      )}
                    >
                      {speed == null ? (
                        "—"
                      ) : (
                        <>
                          {estimatedSpeed != null && "≈"}
                          {speed}
                          <span className="ms-0.5 text-badge font-normal text-fg-3">
                            tok/s
                          </span>
                        </>
                      )}
                    </td>
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
