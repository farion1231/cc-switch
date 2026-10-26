import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  SegmentedControl,
  type SegmentedItem,
} from "@/components/ui/segmented-control";
import { useModelStats } from "@/lib/query/usage";
import { UsageShareChart } from "./UsageShareChart";
import { ModelCacheBreakdown } from "./ModelCacheBreakdown";
import { BreakdownQueryState } from "./BreakdownQueryState";
import { cacheHitLabel } from "./cacheStats";
import {
  fmtInt,
  formatTokensCompact,
  getLocaleFromLanguage,
  getResolvedLang,
  parseFiniteNumber,
} from "./format";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import type { ModelStats, UsageRangeSelection } from "@/types/usage";

type ShareMetric = "tokens" | "cost" | "requests" | "cache";

interface ProviderUsageBreakdownProps {
  providerId: string;
  providerAppType: string;
  providerName: string;
  projectKey?: string;
  projectKeys?: string[];
  model?: string;
  range: UsageRangeSelection;
  refreshIntervalMs: number;
  colSpan: number;
}

/** 供应商展开后的模型占比视图。 */
export function ProviderUsageBreakdown({
  providerId,
  providerAppType,
  providerName,
  projectKey,
  projectKeys,
  model,
  range,
  refreshIntervalMs,
  colSpan,
}: ProviderUsageBreakdownProps) {
  const { t, i18n } = useTranslation();
  const { money } = useMoneyFormat();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const [metric, setMetric] = useState<ShareMetric>("tokens");
  const modelQuery = useModelStats(
    range,
    { providerId, providerAppType, projectKey, projectKeys, model },
    { refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false },
  );
  const stats = modelQuery.data;

  const formatValue = (value: number) => {
    if (metric === "requests") return fmtInt(value, locale);
    if (metric === "cost") return money(value, 2);
    return formatTokensCompact(value, locale);
  };

  const data = useMemo(
    () =>
      (stats ?? []).map((stat: ModelStats) => ({
        key: stat.model,
        label: stat.model,
        value:
          metric === "requests"
            ? stat.requestCount
            : metric === "cost"
              ? (parseFiniteNumber(stat.totalCost) ?? 0)
              : stat.totalTokens,
        detail: t("usage.share.requestCacheHit", {
          value: fmtInt(stat.requestCount, locale),
          rate: cacheHitLabel(stat),
        }),
      })),
    [stats, metric, locale, t],
  );

  const metricItems: SegmentedItem<ShareMetric>[] = [
    { value: "tokens", label: t("usage.share.metric.tokens") },
    { value: "cost", label: t("usage.share.metric.cost") },
    { value: "requests", label: t("usage.share.metric.requests") },
    { value: "cache", label: t("usage.share.metric.cache") },
  ];

  return (
    <tr>
      <td colSpan={colSpan} className="border-b border-border bg-subtle/20 p-0">
        <div className="flex flex-col gap-4 px-4 py-4">
          <div className="flex flex-wrap items-center gap-2">
            <SegmentedControl
              items={metricItems}
              value={metric}
              onValueChange={setMetric}
              aria-label={t("usage.share.metricLabel")}
              size="sm"
            />
          </div>
          {modelQuery.isLoading ||
          modelQuery.isPlaceholderData ||
          (modelQuery.isError && !modelQuery.data) ? (
            <BreakdownQueryState
              loading={modelQuery.isLoading || modelQuery.isPlaceholderData}
              retry={() => void modelQuery.refetch()}
            />
          ) : metric === "cache" ? (
            <ModelCacheBreakdown
              title={t("usage.cacheBreakdown.modelsIn", {
                provider: providerName,
              })}
              stats={stats ?? []}
            />
          ) : (
            <UsageShareChart
              title={t("usage.share.modelsIn", { provider: providerName })}
              ariaLabel={t("usage.share.modelsAria")}
              data={data}
              formatValue={formatValue}
              emptyLabel={t("usage.noData")}
              totalLabel={t("usage.share.total")}
              otherLabel={t("usage.share.other")}
              className="max-w-2xl"
            />
          )}
        </div>
      </td>
    </tr>
  );
}
