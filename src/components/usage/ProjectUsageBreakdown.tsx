import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import {
  SegmentedControl,
  type SegmentedItem,
} from "@/components/ui/segmented-control";
import { Button } from "@/components/ui/button";
import { useModelStats, useProviderStats } from "@/lib/query/usage";
import { getUsageProviderLabel } from "./providerLabel";
import { UsageShareChart } from "./UsageShareChart";
import { ModelCacheBreakdown } from "./ModelCacheBreakdown";
import { BreakdownQueryState } from "./BreakdownQueryState";
import { cacheHitLabel, getCacheStats } from "./cacheStats";
import {
  fmtInt,
  formatTokensCompact,
  getLocaleFromLanguage,
  getResolvedLang,
  parseFiniteNumber,
} from "./format";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import type {
  ModelStats,
  ProviderStats,
  UsageRangeSelection,
} from "@/types/usage";

type ShareMetric = "tokens" | "cost" | "requests" | "cache";

interface ShareStatLike {
  requestCount: number;
  totalTokens: number;
  totalCost: string;
}

function metricValue(stat: ShareStatLike, metric: ShareMetric): number {
  if (metric === "requests") return stat.requestCount;
  if (metric === "cost") return parseFiniteNumber(stat.totalCost) ?? 0;
  return stat.totalTokens;
}

interface ProjectUsageBreakdownProps {
  projectKey: string;
  range: UsageRangeSelection;
  appType?: string;
  providerId?: string;
  providerAppType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
  colSpan: number;
}

/** 项目展开后的占比视图：供应商占比 + 模型占比，避免逐层缩进的表格。 */
export function ProjectUsageBreakdown({
  projectKey,
  range,
  appType,
  providerId,
  providerAppType,
  providerName,
  model,
  refreshIntervalMs,
  colSpan,
}: ProjectUsageBreakdownProps) {
  const { t, i18n } = useTranslation();
  const { money } = useMoneyFormat();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const [metric, setMetric] = useState<ShareMetric>("tokens");
  const scopeKey = JSON.stringify([
    projectKey,
    appType,
    providerId,
    providerAppType,
    providerName,
    model,
    range.preset,
    range.customStartDate,
    range.customEndDate,
    range.liveEndTime,
  ]);
  const [selection, setSelectedProvider] = useState<{
    scopeKey: string;
    key: string;
    providerId: string;
    providerAppType: string;
    label: string;
  } | null>(null);

  const providerQuery = useProviderStats(
    range,
    { appType, projectKey, providerId, providerAppType, providerName, model },
    { refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false },
  );
  const selectedProvider =
    selection?.scopeKey === scopeKey &&
    (!providerQuery.data ||
      providerQuery.isPlaceholderData ||
      providerQuery.data.some(
        (stat) =>
          stat.providerId === selection.providerId &&
          stat.appType === selection.providerAppType,
      ))
      ? selection
      : null;
  useEffect(() => {
    if (selection && !selectedProvider) setSelectedProvider(null);
  }, [selection, selectedProvider]);
  const modelQuery = useModelStats(
    range,
    {
      appType,
      projectKey,
      providerId: selectedProvider?.providerId ?? providerId,
      providerAppType: selectedProvider?.providerAppType ?? providerAppType,
      providerName,
      model,
    },
    { refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false },
  );

  const formatValue = (value: number) => {
    if (metric === "requests") return fmtInt(value, locale);
    if (metric === "cost") return money(value, 2);
    return formatTokensCompact(value, locale);
  };

  const providerData = useMemo(
    () =>
      (providerQuery.data ?? []).map((stat: ProviderStats) => ({
        key: `${stat.appType}:${stat.providerId}`,
        providerId: stat.providerId,
        providerAppType: stat.appType,
        label: getUsageProviderLabel(stat.providerName, t).label,
        value:
          metric === "cache"
            ? (getCacheStats(stat)?.input ?? 0)
            : metricValue(stat, metric),
        detail: t("usage.share.cacheHit", { rate: cacheHitLabel(stat) }),
      })),
    [providerQuery.data, metric, t],
  );

  const modelData = useMemo(
    () =>
      (modelQuery.data ?? []).map((stat: ModelStats) => ({
        key: stat.model,
        label: stat.model,
        value: metricValue(stat, metric),
        detail: t("usage.share.requestCacheHit", {
          value: fmtInt(stat.requestCount, locale),
          rate: cacheHitLabel(stat),
        }),
      })),
    [modelQuery.data, metric, locale, t],
  );

  const metricItems: SegmentedItem<ShareMetric>[] = [
    { value: "tokens", label: t("usage.share.metric.tokens") },
    { value: "cost", label: t("usage.share.metric.cost") },
    { value: "requests", label: t("usage.share.metric.requests") },
    { value: "cache", label: t("usage.share.metric.cache") },
  ];

  const modelsTitle = selectedProvider
    ? t(
        metric === "cache"
          ? "usage.cacheBreakdown.modelsIn"
          : "usage.share.modelsIn",
        { provider: selectedProvider.label },
      )
    : t(
        metric === "cache"
          ? "usage.cacheBreakdown.models"
          : "usage.share.models",
      );

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
            {selectedProvider ? (
              <Button
                type="button"
                variant="quiet"
                size="sm"
                className="gap-1"
                onClick={() => setSelectedProvider(null)}
              >
                <X className="h-3.5 w-3.5" />
                {t("usage.share.allProviders")}
              </Button>
            ) : null}
          </div>
          <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
            {providerQuery.isLoading ||
            providerQuery.isPlaceholderData ||
            (providerQuery.isError && !providerQuery.data) ? (
              <BreakdownQueryState
                loading={
                  providerQuery.isLoading || providerQuery.isPlaceholderData
                }
                retry={() => void providerQuery.refetch()}
              />
            ) : (
              <UsageShareChart
                title={t(
                  metric === "cache"
                    ? "usage.cacheBreakdown.providersInput"
                    : "usage.share.providers",
                )}
                ariaLabel={t("usage.share.providersAria")}
                data={providerData}
                formatValue={formatValue}
                emptyLabel={t("usage.noData")}
                totalLabel={t("usage.share.total")}
                otherLabel={t("usage.share.other")}
                selectedKey={selectedProvider?.key}
                maxSlices={
                  metric === "cache"
                    ? Math.max(providerData.length, 1)
                    : undefined
                }
                includeZeroValues={metric === "cache"}
                onSelect={(key) => {
                  const found = providerData.find((item) => item.key === key);
                  setSelectedProvider((previous) =>
                    previous?.key === key
                      ? null
                      : found
                        ? {
                            scopeKey,
                            key,
                            providerId: found.providerId,
                            providerAppType: found.providerAppType,
                            label: found.label,
                          }
                        : null,
                  );
                }}
              />
            )}
            {modelQuery.isLoading ||
            modelQuery.isPlaceholderData ||
            (modelQuery.isError && !modelQuery.data) ? (
              <BreakdownQueryState
                loading={modelQuery.isLoading || modelQuery.isPlaceholderData}
                retry={() => void modelQuery.refetch()}
              />
            ) : metric === "cache" ? (
              <ModelCacheBreakdown
                title={modelsTitle}
                stats={modelQuery.data ?? []}
              />
            ) : (
              <UsageShareChart
                title={modelsTitle}
                ariaLabel={t("usage.share.modelsAria")}
                data={modelData}
                formatValue={formatValue}
                emptyLabel={t("usage.noData")}
                totalLabel={t("usage.share.total")}
                otherLabel={t("usage.share.other")}
              />
            )}
          </div>
        </div>
      </td>
    </tr>
  );
}
