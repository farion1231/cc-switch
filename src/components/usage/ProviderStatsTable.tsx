import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useProviderStats } from "@/lib/query/usage";
import { useProxyStatusQuery } from "@/lib/query/proxy";
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
import { getAppLabel } from "@/config/appConfig";
import type { UsageRangeSelection } from "@/types/usage";

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
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const { data: stats, isLoading } = useProviderStats(
    range,
    { appType, providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );
  // 推理流是实时数据：useProxyStatusQuery 在代理运行时自带 2s 轮询，
  // 因此跟随代理状态自动更新，无需本面板的刷新间隔参与。
  const { data: proxyStatus } = useProxyStatusQuery();
  // 在飞条数按 (app_type, provider_id) 计，不区分模型；筛了模型时给个 * 提示
  // 数字覆盖全模型，免得读者把它当成"这个模型当前有几条流"。
  const inFlightByApp = proxyStatus?.in_flight_by_provider ?? {};
  const modelFiltered = typeof model === "string" && model.trim() !== "";

  // 只在真实的历史行上附带在飞条数，绝不为"有在飞但无历史"的供应商补零值行：
  // 每一列都描述所选时间范围内的记录用量，凭空拼出的"0 请求 / 0 token / $0"行
  // 每列都在撒谎。此刻在飞但范围内还没有任何完成请求的供应商，本来就不属于
  // 统计表——实时状态请看代理面板。
  const inFlightFor = (appType: string, providerId: string) =>
    inFlightByApp[appType]?.[providerId] ?? 0;

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
              <th className={usageTable.thEnd} title={t("usage.inFlightHint")}>
                {t("usage.inFlight")}
              </th>
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
              pagination.pageRows.map((stat) => {
                const provider = getUsageProviderLabel(stat.providerName, t);
                const inFlight = inFlightFor(stat.appType, stat.providerId);
                return (
                  <tr
                    key={`${stat.appType}\u0000${stat.providerId}`}
                    className={usageTable.row}
                  >
                    <td className={usageTable.td}>
                      <span
                        className="block max-w-[260px] truncate"
                        title={usageProviderTitle(provider)}
                      >
                        {provider.label}
                      </span>
                      {/* providers 主键是 (id, app_type)：同一个 id 会在多个应用下各出一行，
                          名称又可能撞名，所以行上必须标出来源应用。 */}
                      <span className="ms-1.5 inline-block rounded-control bg-subtle px-1 py-0.5 align-middle text-badge font-normal text-fg-3">
                        {getAppLabel(stat.appType)}
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
                      {inFlight}
                      {modelFiltered && inFlight > 0 ? (
                        <span
                          className="ms-1 text-fg-3"
                          title={t("usage.inFlightUnfilteredHint")}
                        >
                          *
                        </span>
                      ) : null}
                    </td>
                    <SuccessSpeedCells stat={stat} />
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
