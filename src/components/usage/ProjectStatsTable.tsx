import { Fragment, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useProjectStats } from "@/lib/query/usage";
import { TablePagination, useClientPagination } from "./TablePagination";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  formatRelativeTime,
  formatTokensCompact,
  getLocaleFromLanguage,
  getResolvedLang,
} from "./format";
import { usageTable } from "./usageTable";
import { ProjectUsageBreakdown } from "./ProjectUsageBreakdown";
import type { UsageRangeSelection } from "@/types/usage";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";

interface ProjectStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  providerAppType?: string;
  providerId?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

function cacheHitLabel(
  cacheableInputTokens: number,
  cacheHitRate: number,
): string {
  return cacheableInputTokens > 0 ? `${(cacheHitRate * 100).toFixed(1)}%` : "—";
}

export function ProjectStatsTable({
  range,
  appType,
  providerAppType,
  providerId,
  providerName,
  model,
  refreshIntervalMs,
}: ProjectStatsTableProps) {
  const { t, i18n } = useTranslation();
  const { money, moneyTitle } = useMoneyFormat();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const { data: stats, isLoading } = useProjectStats(
    range,
    { appType, providerAppType, providerId, providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  const rows = useMemo(
    () => [...(stats ?? [])].sort((a, b) => b.totalTokens - a.totalTokens),
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
      model,
    ]),
  );

  const toggle = (projectKey: string) => {
    setExpanded((previous) => {
      const next = new Set(previous);
      if (next.has(projectKey)) {
        next.delete(projectKey);
      } else {
        next.add(projectKey);
      }
      return next;
    });
  };

  if (isLoading) {
    return <div className={usageTable.skeleton} />;
  }

  const colSpan = 8;

  return (
    <div className="flex flex-col">
      <div className={usageTable.scroller}>
        <table
          className={cn(usageTable.table, "min-w-[760px]")}
          aria-label={t("usage.tabs.projects")}
        >
          <thead>
            <tr className={usageTable.headRow}>
              <th className={usageTable.th}>{t("usage.project")}</th>
              <th className={usageTable.th}>{t("usage.projects.apps")}</th>
              <th className={usageTable.th}>{t("usage.projects.providers")}</th>
              <th className={usageTable.th}>{t("usage.requests")}</th>
              <th className={usageTable.th}>{t("usage.tokens")}</th>
              <th className={usageTable.th}>{t("usage.cost")}</th>
              <th className={usageTable.th}>{t("usage.cacheHitRate")}</th>
              <th className={usageTable.th}>
                {t("usage.projects.lastActive")}
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 ? (
              <tr>
                <td colSpan={colSpan} className={usageTable.empty}>
                  {t("usage.noData")}
                </td>
              </tr>
            ) : (
              pagination.pageRows.map((stat) => {
                const isOpen = expanded.has(stat.projectKey);
                return (
                  <Fragment key={stat.projectKey || "__unknown__"}>
                    <tr className={usageTable.row}>
                      <td className={usageTable.td}>
                        <button
                          type="button"
                          className="flex max-w-[300px] items-center gap-1.5 text-start"
                          onClick={() => toggle(stat.projectKey)}
                          aria-expanded={isOpen}
                          title={stat.projectPath || stat.projectName}
                        >
                          {isOpen ? (
                            <ChevronDown className="h-3.5 w-3.5 shrink-0 text-fg-2" />
                          ) : (
                            <ChevronRight className="h-3.5 w-3.5 shrink-0 text-fg-2" />
                          )}
                          <span className="truncate">
                            {stat.projectName ||
                              stat.projectKey ||
                              t("usage.projectFilter.unknown")}
                          </span>
                        </button>
                      </td>
                      <td className={usageTable.td}>{stat.appCount}</td>
                      <td className={usageTable.td}>{stat.providerCount}</td>
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
                        title={moneyTitle(stat.totalCost, 6)}
                      >
                        {money(stat.totalCost, 2)}
                      </td>
                      <td className={usageTable.td}>
                        {cacheHitLabel(
                          stat.cacheableInputTokens,
                          stat.cacheHitRate,
                        )}
                      </td>
                      <td className={usageTable.td}>
                        {stat.lastActiveAt > 0
                          ? formatRelativeTime(stat.lastActiveAt * 1000, t)
                          : "—"}
                      </td>
                    </tr>
                    {isOpen ? (
                      <ProjectUsageBreakdown
                        projectKey={stat.projectKey}
                        range={range}
                        appType={appType}
                        refreshIntervalMs={refreshIntervalMs}
                        colSpan={colSpan}
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
