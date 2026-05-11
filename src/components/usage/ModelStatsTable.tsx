import { useTranslation } from "react-i18next";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useModelStats } from "@/lib/query/usage";
import { fmtInt, fmtUsd, getLocaleFromLanguage } from "./format";
import type { UsageRangeSelection } from "@/types/usage";

interface ModelStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  refreshIntervalMs: number;
}

function compactToken(value: number, locale: string): string {
  if (value >= 1_000_000) {
    return `${(value / 1_000_000).toLocaleString(locale, {
      maximumFractionDigits: 1,
    })}M`;
  }

  if (value >= 1_000) {
    return `${(value / 1_000).toLocaleString(locale, {
      maximumFractionDigits: 1,
    })}K`;
  }

  return fmtInt(value, locale);
}

export function ModelStatsTable({
  range,
  appType,
  refreshIntervalMs,
}: ModelStatsTableProps) {
  const { t, i18n } = useTranslation();
  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);
  const { data: stats, isLoading } = useModelStats(range, appType, {
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
  });

  if (isLoading) {
    return (
      <div className="h-[360px] animate-pulse rounded-xl border border-border/50 bg-card/40" />
    );
  }

  return (
    <div className="overflow-hidden rounded-xl border border-border/50 bg-card/40 backdrop-blur-sm">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{t("usage.model", "模型")}</TableHead>
            <TableHead className="text-right">
              {t("usage.requests", "请求数")}
            </TableHead>
            <TableHead className="text-right">
              {t("usage.inputTokens", "输入")}
            </TableHead>
            <TableHead className="text-right">
              {t("usage.outputTokens", "输出")}
            </TableHead>
            <TableHead className="text-right">
              {t("usage.cacheReadTokens", "缓存命中")}
            </TableHead>
            <TableHead className="text-right">
              {t("usage.totalCost", "总成本")}
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {stats?.length === 0 ? (
            <TableRow>
              <TableCell
                colSpan={6}
                className="text-center text-muted-foreground"
              >
                {t("usage.noData", "暂无数据")}
              </TableCell>
            </TableRow>
          ) : (
            stats?.map((stat) => (
              <TableRow key={stat.model}>
                <TableCell className="max-w-[280px] truncate font-mono text-sm">
                  {stat.model}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {fmtInt(stat.requestCount, locale)}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {compactToken(stat.totalInputTokens, locale)}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {compactToken(stat.totalOutputTokens, locale)}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {compactToken(stat.totalCacheReadTokens, locale)}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {fmtUsd(stat.totalCost, 4)}
                </TableCell>
              </TableRow>
            ))
          )}
        </TableBody>
      </Table>
    </div>
  );
}
