import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useModelStats } from "@/lib/query/usage";
import { fmtUsd } from "./format";
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
  const { t } = useTranslation();
  const [sort, setSort] = useState<SortState>(DEFAULT_SORT);
  const { data: stats, isLoading } = useModelStats(
    range,
    { appType, providerName, model },
    { refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false },
  );

  const sortedStats = useMemo(
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
    return <div className="h-[400px] animate-pulse rounded bg-gray-100" />;
  }

  const sortableColumns: ReadonlyArray<readonly [SortKey, string]> = [
    ["requestCount", t("usage.requests")],
    ["totalTokens", t("usage.tokens")],
    ["totalCost", t("usage.totalCost")],
    ["avgCostPerRequest", t("usage.avgCost")],
  ];

  return (
    <div className="rounded-lg border border-border/50 bg-card/40 backdrop-blur-sm overflow-hidden">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{t("usage.model")}</TableHead>
            {sortableColumns.map(([key, label]) => (
              <TableHead
                key={key}
                className="text-right"
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
                  className="inline-flex items-center gap-1 hover:text-foreground"
                  onClick={() => toggleSort(key)}
                >
                  {label}
                  {sortIcon(key)}
                </button>
              </TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {sortedStats.length === 0 ? (
            <TableRow>
              <TableCell
                colSpan={5}
                className="text-center text-muted-foreground"
              >
                {t("usage.noData")}
              </TableCell>
            </TableRow>
          ) : (
            sortedStats.map((stat) => (
              <TableRow key={stat.model}>
                <TableCell className="font-mono text-sm">
                  {stat.model}
                </TableCell>
                <TableCell className="text-right">
                  {stat.requestCount.toLocaleString()}
                </TableCell>
                <TableCell className="text-right">
                  {stat.totalTokens.toLocaleString()}
                </TableCell>
                <TableCell className="text-right">
                  {fmtUsd(stat.totalCost, 4)}
                </TableCell>
                <TableCell className="text-right">
                  {fmtUsd(stat.avgCostPerRequest, 6)}
                </TableCell>
              </TableRow>
            ))
          )}
        </TableBody>
      </Table>
    </div>
  );
}
