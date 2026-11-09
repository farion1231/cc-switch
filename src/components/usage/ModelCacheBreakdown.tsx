import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { HelpTip } from "@/components/ui/help-tip";
import type { ModelStats } from "@/types/usage";
import { getCacheStats } from "./cacheStats";
import { UsageShareChart } from "./UsageShareChart";
import {
  fmtInt,
  formatTokensCompact,
  getLocaleFromLanguage,
  getResolvedLang,
} from "./format";

export function ModelCacheBreakdown({
  stats,
  title,
}: {
  stats: ModelStats[];
  title: string;
}) {
  const { t, i18n } = useTranslation();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const rows = useMemo(
    () =>
      stats
        .map((stat) => ({ stat, cache: getCacheStats(stat) }))
        .sort(
          (a, b) =>
            (b.cache?.input ?? -1) - (a.cache?.input ?? -1) ||
            a.stat.model.localeCompare(b.stat.model),
        ),
    [stats],
  );
  const totals = rows.reduce(
    (sum, row) => ({
      read: sum.read + (row.cache?.read ?? 0),
      input: sum.input + (row.cache?.input ?? 0),
      missing: sum.missing + (row.cache == null ? 1 : 0),
    }),
    { read: 0, input: 0, missing: 0 },
  );
  const percent = (rate: number | null) =>
    rate == null ? "—" : `${(rate * 100).toFixed(1)}%`;

  return (
    <section aria-label={title} className="flex min-w-0 flex-col gap-3">
      {rows.length === 0 ? (
        <p className="m-0 py-10 text-center text-caption text-fg-3">
          {t("usage.noData")}
        </p>
      ) : (
        <>
          <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
            <span className="text-caption text-fg-2">
              {t("usage.cacheBreakdown.weightedRate")}
            </span>
            <strong className="text-title tabular-nums text-fg-1">
              {percent(totals.input > 0 ? totals.read / totals.input : null)}
            </strong>
            <span
              className="text-badge tabular-nums text-fg-3"
              title={
                totals.missing === rows.length
                  ? undefined
                  : `${fmtInt(totals.read, locale)} / ${fmtInt(totals.input, locale)}`
              }
            >
              {totals.missing === rows.length
                ? t("usage.cacheBreakdown.missing")
                : t("usage.cacheBreakdown.readOfInput", {
                    read: formatTokensCompact(totals.read, locale),
                    input: formatTokensCompact(totals.input, locale),
                  })}
            </span>
            <HelpTip title={t("usage.cacheHitRate")}>
              {t("usage.cacheBreakdown.help")}
            </HelpTip>
          </div>
          {totals.missing > 0 ? (
            <p role="note" className="m-0 text-badge text-fg-3">
              {t("usage.cacheBreakdown.excluded", { count: totals.missing })}
            </p>
          ) : null}
          <UsageShareChart
            title={title}
            ariaLabel={t("usage.cacheBreakdown.models")}
            data={rows
              .filter(({ cache }) => cache != null)
              .map(({ stat, cache }) => ({
                key: stat.model,
                label: stat.model,
                value: cache?.input ?? 0,
                detail: t("usage.share.requestCacheHit", {
                  value: fmtInt(stat.requestCount, locale),
                  rate: percent(cache?.rate ?? null),
                }),
              }))}
            formatValue={(value) => formatTokensCompact(value, locale)}
            emptyLabel={t("usage.cacheBreakdown.missing")}
            totalLabel={t("usage.inputTokens")}
            otherLabel={t("usage.share.other")}
            maxSlices={rows.length}
            includeZeroValues
          />
        </>
      )}
    </section>
  );
}
