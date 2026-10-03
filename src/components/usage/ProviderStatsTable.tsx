import { useTranslation } from "react-i18next";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useProviderStats } from "@/lib/query/usage";
import { useProxyStatusQuery } from "@/lib/query/proxy";
import { fmtUsd } from "./format";
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
  const { t } = useTranslation();
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
  const providerFiltered =
    typeof providerName === "string" && providerName.trim() !== "";

  // 只在真实的历史行上附带在飞条数，绝不为"有在飞但无历史"的供应商补零值行：
  // 每一列都描述所选时间范围内的记录用量，凭空拼出的"0 请求 / 0 token / $0"行
  // 每列都在撒谎。
  const inFlightFor = (appType: string, providerId: string) =>
    inFlightByApp[appType]?.[providerId] ?? 0;

  // 空状态提示里的在飞条数必须与面板当前口径一致，否则又造出跨口径的数字：
  // - 应用筛选按后端折叠口径匹配（claude-desktop 归入 claude）；
  // - 模型/来源筛选无法在 provider 粒度收窄在飞条数，此时不提示。
  const scopeMatchesApp = (app: string) =>
    appType == null ||
    appType === "all" ||
    app === appType ||
    (appType === "claude" && app === "claude-desktop");
  let inFlightInScope = 0;
  if (!modelFiltered && !providerFiltered) {
    for (const [app, perProvider] of Object.entries(inFlightByApp)) {
      if (!scopeMatchesApp(app)) continue;
      for (const count of Object.values(perProvider ?? {})) {
        if (count > 0) inFlightInScope += count;
      }
    }
  }

  if (isLoading) {
    return <div className="h-[400px] animate-pulse rounded bg-gray-100" />;
  }

  return (
    <div className="rounded-lg border border-border/50 bg-card/40 backdrop-blur-sm overflow-hidden">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{t("usage.provider", "Provider")}</TableHead>
            <TableHead className="text-right">
              {t("usage.requests", "请求数")}
            </TableHead>
            <TableHead className="text-right">
              {t("usage.tokens", "Tokens")}
            </TableHead>
            <TableHead className="text-right">
              {t("usage.cost", "成本")}
            </TableHead>
            <TableHead className="text-right">
              {t("usage.successRate", "成功率")}
            </TableHead>
            <TableHead className="text-right">
              {t("usage.avgLatency", "平均延迟")}
            </TableHead>
            <TableHead className="text-right" title={t("usage.inFlightHint")}>
              {t("usage.inFlight", "推理流")}
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {stats?.length === 0 ? (
            <TableRow>
              <TableCell
                colSpan={7}
                className="text-center text-muted-foreground"
              >
                {t("usage.noData", "暂无数据")}
                {inFlightInScope > 0 ? (
                  <p className="mt-1 text-xs">
                    {t("usage.inFlightNoRowsYet", { streams: inFlightInScope })}
                  </p>
                ) : null}
              </TableCell>
            </TableRow>
          ) : (
            stats?.map((stat) => {
              const inFlight = inFlightFor(stat.appType, stat.providerId);
              return (
                <TableRow key={`${stat.appType}\u0000${stat.providerId}`}>
                  <TableCell className="font-medium">
                    {stat.providerName || stat.providerId}
                    {/* providers 主键是 (id, app_type)：同一个 id 会在多个应用下各出一行，
                        名称又可能撞名，所以行上必须标出来源应用。 */}
                    <span className="ml-1.5 rounded bg-muted px-1 py-0.5 align-middle text-[10px] font-normal text-muted-foreground">
                      {getAppLabel(stat.appType)}
                    </span>
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
                    {stat.successRate.toFixed(1)}%
                  </TableCell>
                  <TableCell className="text-right">
                    {stat.avgLatencyMs}ms
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {inFlight > 0 ? (
                      // 实时状态要有"活"的样子：脉冲点 + 悬浮提示，别让读者
                      // 把它读成本时段的统计值；没有流量时置灰，避免一列 0
                      // 看起来像数据。
                      <span
                        className="inline-flex items-center justify-end gap-1.5"
                        title={t("usage.inFlightHint")}
                      >
                        <span className="inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-500" />
                        {inFlight}
                      </span>
                    ) : (
                      <span className="text-muted-foreground">{inFlight}</span>
                    )}
                    {modelFiltered && inFlight > 0 ? (
                      <span
                        className="ml-1 text-muted-foreground"
                        title={t("usage.inFlightUnfilteredHint")}
                      >
                        *
                      </span>
                    ) : null}
                  </TableCell>
                </TableRow>
              );
            })
          )}
        </TableBody>
      </Table>
    </div>
  );
}
