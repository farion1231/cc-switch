import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { HelpTip } from "@/components/ui/help-tip";
import type { ModelStats } from "@/types/usage";
import { getCacheStats } from "./cacheStats";
import {
  fmtInt,
  formatTokensCompact,
  getLocaleFromLanguage,
  getResolvedLang,
} from "./format";

const PARTS = [
  { key: "read", color: "var(--chart-1)" },
  { key: "write", color: "var(--chart-3)" },
  { key: "fresh", color: "var(--chart-2)" },
] as const;

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
      <h3 className="m-0 text-caption font-semibold text-fg-2">{title}</h3>
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
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-badge text-fg-3">
            {PARTS.map((part) => (
              <span key={part.key} className="inline-flex items-center gap-1.5">
                <span
                  aria-hidden="true"
                  className="size-2 rounded-[2px]"
                  style={{ background: part.color }}
                />
                {t(`usage.cacheBreakdown.${part.key}`)}
              </span>
            ))}
            <span className="ms-auto">{t("usage.cacheBreakdown.order")}</span>
          </div>
          <ul
            aria-label={t("usage.cacheBreakdown.models")}
            tabIndex={0}
            className="m-0 max-h-80 list-none space-y-4 overflow-y-auto p-0 pe-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            {rows.map(({ stat, cache }) => (
              <li key={stat.model} className="flex min-w-0 flex-col gap-1.5">
                <div className="flex min-w-0 items-baseline gap-2 text-caption">
                  <span
                    className="min-w-0 flex-1 truncate text-fg-1"
                    title={stat.model}
                  >
                    {stat.model}
                  </span>
                  <span className="shrink-0 text-badge tabular-nums text-fg-3">
                    {t("usage.share.requestCount", {
                      value: fmtInt(stat.requestCount, locale),
                    })}
                  </span>
                  <strong className="w-14 shrink-0 text-end tabular-nums text-fg-1">
                    {percent(cache?.rate ?? null)}
                  </strong>
                </div>
                <div
                  aria-hidden="true"
                  className="flex h-2 overflow-hidden rounded-full bg-subtle"
                >
                  {cache && cache.input > 0
                    ? PARTS.map((part) => (
                        <span
                          key={part.key}
                          style={{
                            width: `${(cache[part.key] / cache.input) * 100}%`,
                            background: part.color,
                          }}
                        />
                      ))
                    : null}
                </div>
                {cache ? (
                  <div className="flex flex-wrap gap-x-3 gap-y-0.5 text-badge tabular-nums text-fg-3">
                    {PARTS.map((part) => (
                      <span
                        key={part.key}
                        title={fmtInt(cache[part.key], locale)}
                      >
                        {t(`usage.cacheBreakdown.${part.key}`)}{" "}
                        {formatTokensCompact(cache[part.key], locale)}
                      </span>
                    ))}
                  </div>
                ) : (
                  <p className="m-0 text-badge text-fg-3">
                    {t("usage.cacheBreakdown.missing")}
                  </p>
                )}
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}
