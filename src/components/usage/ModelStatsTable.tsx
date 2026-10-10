import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react";
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
import { SuccessSpeedCells, SuccessSpeedHeaders } from "./statsColumns";
import type { UsageRangeSelection } from "@/types/usage";

type SortKey =
  | "requestCount"
  | "totalTokens"
  | "totalCost"
  | "avgCostPerRequest";
type SortDirection = "asc" | "desc";
type SortState = { key: SortKey; direction: SortDirection };

const DEFAULT_SORT: SortState = { key: "totalCost", direction: "desc" };

const numericValue = (value: number | string) => {
  const parsed = typeof value === "number" ? value : Number.parseFloat(value);
  return Number.isFinite(parsed) ? parsed : 0;
};

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
  const [sort, setSort] = useState<SortState>(DEFAULT_SORT);
  const { data: stats, isLoading } = useModelStats(
    range,
    { appType, providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  const rows = useMemo(
    () =>
      [...(stats ?? [])].sort((left, right) => {
        const difference =
          numericValue(left[sort.key]) - numericValue(right[sort.key]);
        if (difference !== 0) {
          return sort.direction === "asc" ? difference : -difference;
        }
        return left.model.localeCompare(right.model);
      }),
    [sort, stats],
  );
  const pagination = useClientPagination(
    rows,
    JSON.stringify([range, appType, providerName, model, sort]),
  );

  const toggleSort = (key: SortKey) => {
    setSort((current) => ({
      key,
      direction:
        current.key === key && current.direction === "desc" ? "asc" : "desc",
    }));
  };

  const sortIcon = (key: SortKey) => {
    if (sort.key !== key) {
      return <ArrowUpDown className="h-3.5 w-3.5" aria-hidden="true" />;
    }
    return sort.direction === "asc" ? (
      <ArrowUp className="h-3.5 w-3.5" aria-hidden="true" />
    ) : (
      <ArrowDown className="h-3.5 w-3.5" aria-hidden="true" />
    );
  };

  if (isLoading) {
    return <div className={usageTable.skeleton} />;
  }

  const sortableColumns: ReadonlyArray<readonly [SortKey, string]> = [
    ["requestCount", t("usage.requests")],
    ["totalTokens", t("usage.tokens")],
    ["totalCost", t("usage.cost")],
    ["avgCostPerRequest", t("usage.avgCost")],
  ];

  return (
    <div className="flex flex-col">
      <div className={usageTable.scroller}>
        <table
          className={cn(usageTable.table, "min-w-[620px]")}
          aria-label={t("usage.modelStats")}
        >
          <thead>
            <tr className={usageTable.headRow}>
              <th className={usageTable.th}>{t("usage.model")}</th>
              {sortableColumns.map(([key, label]) => (
                <th
                  key={key}
                  className={usageTable.thEnd}
                  aria-sort={
                    sort.key === key
                      ? sort.direction === "asc"
                        ? "ascending"
                        : "descending"
                      : "none"
                  }
                >
                  <button
                    type="button"
                    className="inline-flex items-center gap-1 hover:text-fg-1"
                    onClick={() => toggleSort(key)}
                  >
                    {label}
                    {sortIcon(key)}
                  </button>
                </th>
              ))}
              <SuccessSpeedHeaders />
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
                    className={cn(usageTable.tdEnd, "font-medium")}
                    title={fmtUsd(stat.avgCostPerRequest, 6)}
                  >
                    {fmtUsd(stat.avgCostPerRequest, 4)}
                  </td>
                  <SuccessSpeedCells stat={stat} />
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
