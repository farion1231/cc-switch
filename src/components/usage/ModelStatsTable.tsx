import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useModelStats } from "@/lib/query/usage";
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
import { PerformanceCells, PerformanceHeaders } from "./statsColumns";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { UsageRangeSelection, UsageModelStat } from "@/types/usage";

interface ModelStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
  snapshot?: {
    data: UsageModelStat[];
    isLoading: boolean;
    resetKey: string;
    grouping?: "day" | "model";
  };
}

export function ModelStatsTable({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
  snapshot,
}: ModelStatsTableProps) {
  const local = useModelStats(
    range,
    { appType, providerName, model },
    {
      enabled: !snapshot,
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );
  return (
    <ModelStatsTableView
      data={snapshot?.data ?? local.data ?? []}
      isLoading={snapshot ? snapshot.isLoading : local.isLoading}
      resetKey={
        snapshot?.resetKey ??
        JSON.stringify([range, appType, providerName, model])
      }
      grouping={snapshot?.grouping}
    />
  );
}
function ModelStatsTableView({
  data,
  isLoading,
  resetKey,
  grouping = "model",
}: {
  data: UsageModelStat[];
  isLoading: boolean;
  resetKey: string;
  grouping?: "day" | "model";
}) {
  const { t, i18n } = useTranslation();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const daily = grouping === "day";
  const rows = useMemo(
    () =>
      [...data].sort((a, b) =>
        daily
          ? (b.date ?? "").localeCompare(a.date ?? "") ||
            (a.appType ?? "").localeCompare(b.appType ?? "") ||
            a.model.localeCompare(b.model)
          : b.requestCount - a.requestCount,
      ),
    [data, daily],
  );
  const pagination = useClientPagination(rows, resetKey);

  if (isLoading) {
    return <div className={usageTable.skeleton} />;
  }

  return (
    <div className="flex flex-col">
      <div className={usageTable.scroller}>
        <table
          className={cn(usageTable.table, "min-w-[620px]")}
          aria-label={t(daily ? "usage.devices.breakdown" : "usage.modelStats")}
        >
          <thead>
            <tr className={usageTable.headRow}>
              {daily && (
                <>
                  <th className={usageTable.th}>{t("usage.devices.date")}</th>
                  <th className={usageTable.th}>{t("usage.devices.app")}</th>
                </>
              )}
              <th className={usageTable.th}>{t("usage.model")}</th>
              {daily && (
                <th className={usageTable.th}>{t("usage.devices.quality")}</th>
              )}
              <th className={usageTable.th}>{t("usage.requests")}</th>
              <th className={usageTable.th}>{t("usage.tokens")}</th>
              <th className={usageTable.th}>{t("usage.cost")}</th>
              {!daily && <PerformanceHeaders />}
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
              pagination.pageRows.map((stat) => (
                <tr
                  key={
                    daily
                      ? `${stat.date}-${stat.appType}-${stat.model}-${stat.verified}`
                      : stat.model
                  }
                  className={usageTable.row}
                >
                  {daily && (
                    <>
                      <td className={usageTable.td}>{stat.date}</td>
                      <td className={usageTable.td}>
                        {APP_DISPLAY_NAME[
                          stat.appType as keyof typeof APP_DISPLAY_NAME
                        ] ?? stat.appType}
                      </td>
                    </>
                  )}
                  <td className={cn(usageTable.td, usageTable.mono)}>
                    <span
                      className="block max-w-[320px] truncate"
                      title={stat.model}
                    >
                      {stat.model}
                    </span>
                  </td>
                  {daily && (
                    <td
                      className={cn(usageTable.td, usageTable.muted)}
                      title={t("usage.devices.qualityHelp")}
                    >
                      {t(
                        stat.verified
                          ? "usage.devices.verifiedShort"
                          : "usage.devices.unverifiedShort",
                      )}
                    </td>
                  )}
                  <td className={usageTable.td}>
                    {fmtInt(stat.requestCount, locale)}
                  </td>
                  <td
                    className={usageTable.td}
                    title={fmtInt(stat.totalTokens, locale)}
                  >
                    {formatTokensCompact(stat.totalTokens, locale)}
                  </td>
                  <td
                    className={cn(usageTable.td, "font-medium")}
                    title={`${fmtUsd(stat.totalCost, 6)} · ${t("usage.avgCost")} ${fmtUsd(stat.avgCostPerRequest, 4)}`}
                  >
                    {stat.estimated && "≈"}
                    {fmtUsd(stat.totalCost, 2)}
                  </td>
                  {!daily && <PerformanceCells stat={stat} />}
                </tr>
              ))
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
