import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useModelStats } from "@/lib/query/usage";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  fmtUsd,
  formatTokensCompact,
  getLocaleFromLanguage,
  getResolvedLang,
} from "./format";
import { usageTable } from "./usageTable";
import type { UsageRangeSelection } from "@/types/usage";

interface ModelStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

export function ModelStatsTable({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: ModelStatsTableProps) {
  const { t, i18n } = useTranslation();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const { data: stats, isLoading } = useModelStats(
    range,
    { appType, providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  const rows = useMemo(
    () => [...(stats ?? [])].sort((a, b) => b.requestCount - a.requestCount),
    [stats],
  );

  if (isLoading) {
    return <div className={usageTable.skeleton} />;
  }

  return (
    <div className={usageTable.scroller}>
      <table
        className={cn(usageTable.table, "min-w-[560px]")}
        aria-label={t("usage.modelStats")}
      >
        <thead>
          <tr className={usageTable.headRow}>
            <th className={usageTable.th}>{t("usage.model")}</th>
            <th className={usageTable.thEnd}>{t("usage.requests")}</th>
            <th className={usageTable.thEnd}>{t("usage.tokens")}</th>
            <th className={usageTable.thEnd}>{t("usage.totalCost")}</th>
            <th className={usageTable.thEnd}>{t("usage.avgCost")}</th>
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 ? (
            <tr>
              <td colSpan={5} className={usageTable.empty}>
                {t("usage.noData")}
              </td>
            </tr>
          ) : (
            rows.map((stat) => (
              <tr key={stat.model} className={usageTable.row}>
                <td className={cn(usageTable.td, usageTable.mono)}>
                  <span
                    className="block max-w-[320px] truncate"
                    title={stat.model}
                  >
                    {stat.model}
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
                <td
                  className={cn(usageTable.tdEnd, "text-fg-2")}
                  title={fmtUsd(stat.avgCostPerRequest, 6)}
                >
                  {fmtUsd(stat.avgCostPerRequest, 4)}
                </td>
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  );
}
