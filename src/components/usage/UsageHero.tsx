import { useTranslation } from "react-i18next";
import { useUsageSummaryByApp } from "@/lib/query/usage";
import { HelpTip } from "@/components/ui/help-tip";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  fmtUsd,
  formatTokensShort,
  getLocaleFromLanguage,
  getResolvedLang,
  parseFiniteNumber,
} from "./format";
import {
  getCacheWriteAvailability,
  type UsageRangeSelection,
  type UsageSummary,
  type UsageSummaryByApp,
} from "@/types/usage";

interface UsageHeroProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
  /** 窄容器下指标卡排成两列 */
  compact?: boolean;
}

/**
 * Combine per-app summaries into a single rolled-up summary.
 *
 * The backend's per-app rows already use fresh-input semantics (cache-inclusive
 * providers have been normalized in SQL), so plain addition is correct here.
 * `cacheHitRate` and `successRate` must be re-derived from the summed counts
 * rather than averaged across rows.
 */
export function aggregateSummaries(items: UsageSummary[]): UsageSummary {
  let totalRequests = 0;
  let successCount = 0;
  let totalCostNum = 0;
  let input = 0;
  let output = 0;
  let cacheCreation = 0;
  let cacheRead = 0;

  for (const s of items) {
    totalRequests += s.totalRequests;
    successCount += Math.round((s.totalRequests * s.successRate) / 100);
    totalCostNum += parseFiniteNumber(s.totalCost) ?? 0;
    input += s.totalInputTokens;
    output += s.totalOutputTokens;
    cacheCreation += s.totalCacheCreationTokens;
    cacheRead += s.totalCacheReadTokens;
  }

  const cacheableInput = input + cacheCreation + cacheRead;
  return {
    totalRequests,
    totalCost: totalCostNum.toFixed(6),
    totalInputTokens: input,
    totalOutputTokens: output,
    totalCacheCreationTokens: cacheCreation,
    totalCacheReadTokens: cacheRead,
    successRate: totalRequests > 0 ? (successCount / totalRequests) * 100 : 0,
    realTotalTokens: input + output + cacheCreation + cacheRead,
    cacheHitRate: cacheableInput > 0 ? cacheRead / cacheableInput : 0,
  };
}

function pickSummary(
  apps: UsageSummaryByApp[],
  appType: string | undefined,
): UsageSummary | undefined {
  if (apps.length === 0) return undefined;
  if (appType) {
    return apps.find((a) => a.appType === appType)?.summary;
  }
  return aggregateSummaries(apps.map((a) => a.summary));
}

/** 卡片右侧的大号装饰符号：粗线条、圆头、低对比，越出卡片边缘被裁掉一部分 */
function PowerGlyph({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 100 100"
      fill="none"
      aria-hidden="true"
      className={className}
    >
      <path
        d="M31 27 A34 34 0 1 0 69 27"
        stroke="currentColor"
        strokeWidth="13"
        strokeLinecap="round"
      />
      <path
        d="M50 10 V44"
        stroke="currentColor"
        strokeWidth="13"
        strokeLinecap="round"
      />
    </svg>
  );
}

function DollarGlyph({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 100 120"
      fill="none"
      aria-hidden="true"
      className={className}
    >
      <path
        d="M50 8 V112"
        stroke="currentColor"
        strokeWidth="13"
        strokeLinecap="round"
      />
      <path
        d="M76 34 C72 22 60 20 50 20 C36 20 26 27 26 39 C26 53 40 56 50 59 C62 62 76 66 76 81 C76 93 64 100 50 100 C38 100 28 96 24 85"
        stroke="currentColor"
        strokeWidth="13"
        strokeLinecap="round"
      />
    </svg>
  );
}

function HeroCard({
  label,
  help,
  value,
  title,
  details,
  glyph: Glyph,
  compact,
}: {
  label: string;
  help?: { title: string; body: string };
  value: string;
  title?: string;
  details: Array<{ label: string; value: string; title?: string }>;
  glyph: (props: { className?: string }) => JSX.Element;
  compact: boolean;
}) {
  return (
    <div
      className={cn(
        "relative min-w-0 overflow-hidden rounded-[14px] border border-border bg-surface",
        compact ? "px-6 py-6" : "px-8 py-8",
      )}
    >
      <Glyph
        className={cn(
          "pointer-events-none absolute text-fg-1 opacity-[0.07]",
          compact
            ? "-end-6 -top-4 h-[200px] w-[200px]"
            : "-end-8 -top-6 h-[300px] w-[300px]",
        )}
      />
      <div className="relative flex min-w-0 items-center gap-0.5">
        <span className="truncate text-[15px] text-fg-2">{label}</span>
        {help && (
          <HelpTip title={help.title} align="end">
            {help.body}
          </HelpTip>
        )}
      </div>
      <div
        className={cn(
          // 行高要和字号写在一起：放在前面会被 cn 合并时的字号类冲掉，大字被裁
          "relative overflow-hidden text-ellipsis whitespace-nowrap font-light tabular-nums tracking-tight text-fg-1",
          compact
            ? "mt-4 text-[44px] leading-[1.15]"
            : "mt-6 text-[72px] leading-[1.1]",
        )}
        title={title}
      >
        {value}
      </div>
      <div
        className={cn(
          "relative flex flex-wrap gap-x-6 gap-y-1 text-[14px] text-fg-3",
          compact ? "mt-4" : "mt-6",
        )}
      >
        {details.map((item) => (
          <span
            key={item.label}
            className="whitespace-nowrap"
            title={item.title}
          >
            {item.label}{" "}
            <span className="tabular-nums text-fg-2">{item.value}</span>
          </span>
        ))}
      </div>
    </div>
  );
}

function StripMetric({
  label,
  value,
  title,
  sub,
  muted,
  help,
}: {
  label: string;
  value: string;
  title?: string;
  sub?: string;
  muted?: boolean;
  help?: { title: string; body: string };
}) {
  return (
    <div className="flex min-w-0 flex-col px-6 py-5">
      <div className="flex min-w-0 items-center gap-0.5">
        <span className="truncate text-[14px] text-fg-3">{label}</span>
        {help && <HelpTip title={help.title}>{help.body}</HelpTip>}
      </div>
      <span
        className={cn(
          "mt-2 truncate text-[24px] font-light leading-[1.2] tabular-nums",
          muted ? "text-fg-3" : "text-fg-1",
        )}
        title={title}
      >
        {value}
      </span>
      {sub && (
        <span className="mt-1 truncate text-caption text-fg-3">{sub}</span>
      )}
    </div>
  );
}

/**
 * 指标区：上面两张大卡（Token 总量、总成本），下面一条六格指标条。
 */
export function UsageHero({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
  compact = false,
}: UsageHeroProps) {
  const { t, i18n } = useTranslation();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));

  const { data, isLoading } = useUsageSummaryByApp(
    range,
    { providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  // No client-side filtering: totals must match the Trend/Logs/Stats below,
  // which all go through the backend's full set of app_types. The
  // KNOWN_APP_TYPES list only governs which filter chips appear.
  const allApps = data ?? [];
  const summary = pickSummary(allApps, appType);

  const cacheWriteState = getCacheWriteAvailability(
    appType ? [appType] : allApps.map((a) => a.appType),
  );

  const input = summary?.totalInputTokens ?? 0;
  const output = summary?.totalOutputTokens ?? 0;
  const cacheWrite = summary?.totalCacheCreationTokens ?? 0;
  const cacheRead = summary?.totalCacheReadTokens ?? 0;
  const realTotal = summary?.realTotalTokens ?? 0;
  const hitRate = summary?.cacheHitRate ?? 0;
  const totalCost = parseFiniteNumber(summary?.totalCost);
  const requests = summary?.totalRequests ?? 0;

  const hitPercent = Math.max(0, Math.min(100, hitRate * 100));
  const hitPercentLabel = hitPercent.toFixed(hitPercent >= 99.95 ? 0 : 1);
  const placeholder = isLoading ? "…" : undefined;

  const cacheWriteHelp =
    cacheWriteState === "na"
      ? t("usage.cacheWriteNotReported")
      : cacheWriteState === "partial"
        ? t("usage.cacheWritePartial")
        : undefined;

  const lang = getResolvedLang(i18n);
  const tokens = (value: number) =>
    placeholder ?? formatTokensShort(value, lang);
  const costPerRequest =
    totalCost != null && requests > 0 ? totalCost / requests : null;
  const costPerMillion =
    totalCost != null && realTotal > 0 ? (totalCost / realTotal) * 1e6 : null;
  const successRate = summary?.successRate ?? 0;

  return (
    <section
      aria-label={t("usage.metrics.label")}
      className="flex shrink-0 flex-col gap-2.5"
    >
      <div
        className={cn("grid gap-2.5", compact ? "grid-cols-1" : "grid-cols-2")}
      >
        <HeroCard
          compact={compact}
          glyph={PowerGlyph}
          label={t("usage.realTotal")}
          help={{
            title: t("usage.metrics.realTotalHelpTitle"),
            body: t("usage.metrics.realTotalHelp"),
          }}
          value={tokens(realTotal)}
          title={fmtInt(realTotal, locale)}
          details={[
            { label: t("usage.freshInput"), value: tokens(input) },
            { label: t("usage.metrics.outputLabel"), value: tokens(output) },
            {
              label: t("usage.metrics.cacheTotal"),
              value: tokens(cacheRead + cacheWrite),
            },
          ]}
        />
        <HeroCard
          compact={compact}
          glyph={DollarGlyph}
          label={t("usage.totalCost")}
          value={
            placeholder ??
            (totalCost == null
              ? "--"
              : `$${totalCost.toLocaleString(locale, {
                  minimumFractionDigits: 2,
                  maximumFractionDigits: 2,
                })}`)
          }
          title={totalCost == null ? undefined : fmtUsd(totalCost, 6)}
          details={[
            {
              label: t("usage.metrics.perRequest"),
              value: costPerRequest == null ? "--" : fmtUsd(costPerRequest, 4),
            },
            {
              label: t("usage.metrics.perMillion"),
              value: costPerMillion == null ? "--" : fmtUsd(costPerMillion, 2),
            },
            {
              label: t("usage.successRate"),
              value: `${successRate.toFixed(successRate >= 99.95 ? 0 : 1)}%`,
            },
          ]}
        />
      </div>

      <div
        className={cn(
          "grid divide-border overflow-hidden rounded-[14px] border border-border bg-surface",
          compact
            ? "grid-cols-3 divide-x [&>*:nth-child(n+4)]:border-t"
            : "grid-cols-6 divide-x",
        )}
      >
        <StripMetric
          label={t("usage.totalRequests")}
          value={placeholder ?? fmtInt(requests, locale)}
        />
        <StripMetric
          label={t("usage.freshInput")}
          value={tokens(input)}
          title={fmtInt(input, locale)}
        />
        <StripMetric
          label={t("usage.metrics.outputLabel")}
          value={tokens(output)}
          title={fmtInt(output, locale)}
        />
        <StripMetric
          label={t("usage.metrics.cacheReadLabel")}
          value={tokens(cacheRead)}
          title={fmtInt(cacheRead, locale)}
        />
        <StripMetric
          label={t("usage.metrics.cacheWriteLabel")}
          value={cacheWriteState === "na" ? "N/A" : tokens(cacheWrite)}
          title={cacheWriteState === "na" ? undefined : fmtInt(cacheWrite)}
          muted={cacheWriteState === "na"}
          help={
            cacheWriteHelp
              ? { title: t("usage.cacheWrite"), body: cacheWriteHelp }
              : undefined
          }
        />
        <StripMetric
          label={t("usage.cacheHitRate")}
          help={{
            title: t("usage.metrics.hitRateHelpTitle"),
            body: t("usage.metrics.hitRateHelp"),
          }}
          value={placeholder ?? `${hitPercentLabel}%`}
          sub={t("usage.metrics.hitRateSub")}
        />
      </div>
    </section>
  );
}
