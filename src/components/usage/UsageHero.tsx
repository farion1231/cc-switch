import { usageCostLabel } from "@/components/usage/format";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";
import { useUsageSummaryByApp } from "@/lib/query/usage";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  formatHitRatePercent,
  formatTokensCompact,
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
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import type { UsageMetric } from "./rememberedChoice";

interface UsageHeroProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
  /** 窄容器下指标排成两列 */
  compact?: boolean;
  /** 选中的指标就是下方趋势图 / 热力图画的指标 */
  metric: UsageMetric;
  onMetricChange: (metric: UsageMetric) => void;
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
    costIncomplete: items.some((item) => item.costIncomplete),
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

/**
 * 指标格：整格是一个按下态按钮（铺满整格、放在内容底下），内容层不接指针事件，
 * 只有「?」接——按钮里不能再套按钮。选中的格子是实心底色，其余透明。
 */
function MetricCell({
  label,
  help,
  value,
  title,
  selected,
  onSelect,
}: {
  label: string;
  help?: { title: string; body: string };
  value: string;
  title?: string;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <div className="group relative min-w-0 px-4 pb-3.5 pt-2.5">
      <button
        type="button"
        aria-pressed={selected}
        aria-label={`${label} ${value}`}
        onClick={onSelect}
        className={cn(
          "absolute inset-0 transition-[background-color,box-shadow] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
          // 选中：底边一道 2px 正文色线，下半像素压住分隔线，和它在同一水平线上；不再用底色
          // 悬停用 group：鼠标在数字上（数字接指针事件、显示完整值）时整格也要变色
          selected
            ? "shadow-[inset_0_-1px_0_var(--text-1),0_1px_0_var(--text-1)]"
            : "group-hover:bg-sidebar",
        )}
      />
      <div className="pointer-events-none relative flex min-w-0 flex-col gap-0.5">
        <div className="flex h-[18px] min-w-0 items-center gap-0.5">
          <span
            className={cn(
              "truncate text-caption",
              selected ? "text-fg-1" : "text-fg-2",
            )}
          >
            {label}
          </span>
          {help && (
            // flex 包一层：行内 span 会让「?」按基线排、比文字低约 1px
            <span className="pointer-events-auto flex">
              <HelpTip title={help.title} align="end">
                {help.body}
              </HelpTip>
            </span>
          )}
        </div>
        {/* 完整数字的提示挂在数字上、往右弹：挂在整格上会弹到格子底下、盖住展开的第二行。
            数字接指针事件才能悬停，点它同样是选这一格 */}
        <HoverTip content={title} side="right">
          <span
            aria-hidden="true"
            onClick={onSelect}
            className={cn(
              "max-w-full self-start truncate text-metric leading-7 tabular-nums",
              title && "pointer-events-auto",
              selected ? "text-fg-1" : "text-fg-2",
            )}
          >
            {value}
          </span>
        </HoverTip>
      </div>
    </div>
  );
}

/**
 * Tokens 的一个组成部分：展开行里的一格，和上面的指标格同列、同内边距；
 * 标签在上、数值在下，「?」跟在标签后面。数值不截断。
 * 不加底色：灰底已经表示「选中」，这一行只靠更小的字号和灰字表示是二级指标。
 */
function SubMetric({
  label,
  value,
  title,
  muted,
  help,
}: {
  label: string;
  value: string;
  title?: string;
  muted?: boolean;
  help?: { title: string; body: string };
}) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5 px-4 py-2.5">
      <span className="flex h-[18px] items-center gap-0.5 whitespace-nowrap text-caption text-fg-3">
        {label}
        {help && <HelpTip title={help.title}>{help.body}</HelpTip>}
      </span>
      <HoverTip content={title} side="right">
        <span
          className={cn(
            "self-start whitespace-nowrap text-section tabular-nums",
            muted ? "font-medium text-fg-3" : "text-fg-2",
          )}
        >
          {value}
        </span>
      </HoverTip>
    </div>
  );
}

/**
 * 指标条（概览面板顶部）：总成本、总请求数、真实消耗 Tokens、缓存命中率 4 格等宽，
 * 点一格切换下方图表的指标。Tokens 的 4 个组成部分（新增输入、输出、缓存写入、缓存命中）
 * 是可选的展示项：默认收起，点分隔线正中的箭头展开成第二行，和上面 4 列对齐。
 */
export function UsageHero({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
  compact = false,
  metric,
  onMetricChange,
}: UsageHeroProps) {
  const { t, i18n } = useTranslation();
  const { money, moneyTitle } = useMoneyFormat();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const [moreOpen, setMoreOpen] = useState(false);
  const moreId = useId();

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

  // 加载中写「…」；这段时间一条请求都没有时，各项都写「—」（和表格的空值一致），
  // 不写 0 / 0.0%：那是「有数据、结果为零」的意思
  const placeholder = isLoading ? "…" : requests === 0 ? "—" : undefined;
  // 命中率的分母（新增输入 + 缓存写入 + 缓存命中）为 0 时没有意义，同样写「—」
  const hitRateKnown = input + cacheWrite + cacheRead > 0;
  const tokens = (value: number) => formatTokensCompact(value, locale);
  // 悬停看完整数字；加载中、没有数据时格子里是「…」「—」，不提示
  const exact = (text: string | undefined) => (placeholder ? undefined : text);
  const columns = compact ? "grid-cols-2" : "grid-cols-4";

  const cacheWriteHelp =
    cacheWriteState === "na"
      ? t("usage.cacheWriteNotReported")
      : cacheWriteState === "partial"
        ? t("usage.cacheWritePartial")
        : undefined;

  return (
    <section
      aria-label={t("usage.metrics.label")}
      className="relative z-10 border-b border-border"
    >
      <div className={cn("grid", columns)}>
        <MetricCell
          label={t("usage.totalCost")}
          value={
            placeholder ??
            (totalCost == null
              ? "—"
              : usageCostLabel(
                  totalCost,
                  summary?.costIncomplete,
                  money(totalCost, 2),
                  t("usage.unpriced"),
                ))
          }
          title={
            summary?.costIncomplete
              ? t("usage.costIncomplete")
              : totalCost == null
                ? undefined
                : exact(moneyTitle(totalCost, 6))
          }
          selected={metric === "cost"}
          onSelect={() => onMetricChange("cost")}
        />
        <MetricCell
          label={t("usage.totalRequests")}
          value={placeholder ?? fmtInt(requests, locale)}
          selected={metric === "requests"}
          onSelect={() => onMetricChange("requests")}
        />
        <MetricCell
          label={t("usage.realTotal")}
          help={{
            title: t("usage.metrics.realTotalHelpTitle"),
            body: t("usage.metrics.realTotalHelp"),
          }}
          value={placeholder ?? tokens(realTotal)}
          title={exact(fmtInt(realTotal, locale))}
          selected={metric === "tokens"}
          onSelect={() => onMetricChange("tokens")}
        />
        <MetricCell
          label={t("usage.cacheHitRate")}
          help={{
            title: t("usage.metrics.hitRateHelpTitle"),
            body: t("usage.metrics.hitRateHelp"),
          }}
          value={
            placeholder ?? (hitRateKnown ? formatHitRatePercent(hitRate) : "—")
          }
          selected={metric === "hitRate"}
          onSelect={() => onMetricChange("hitRate")}
        />
      </div>

      {/* 展开 / 收起：0fr ↔ 1fr 过渡高度；收起时整行 inert，里面的「?」不进 Tab 顺序 */}
      <div
        id={moreId}
        className={cn(
          "grid transition-[grid-template-rows] duration-200 ease-out motion-reduce:transition-none",
          moreOpen ? "grid-rows-[1fr]" : "grid-rows-[0fr]",
        )}
        {...(moreOpen ? {} : { inert: "" })}
      >
        <div className="min-h-0 overflow-hidden">
          <div className={cn("grid border-t border-border", columns)}>
            <SubMetric
              label={t("usage.freshInput")}
              value={placeholder ?? tokens(input)}
              title={exact(fmtInt(input, locale))}
            />
            <SubMetric
              label={t("usage.outputTokens")}
              value={placeholder ?? tokens(output)}
              title={exact(fmtInt(output, locale))}
            />
            <SubMetric
              label={t("usage.logColumns.cacheWrite")}
              value={
                cacheWriteState === "na"
                  ? "N/A"
                  : (placeholder ?? tokens(cacheWrite))
              }
              title={
                cacheWriteState === "na"
                  ? undefined
                  : exact(fmtInt(cacheWrite, locale))
              }
              muted={cacheWriteState === "na"}
              help={
                cacheWriteHelp
                  ? {
                      title: t("usage.logColumns.cacheWrite"),
                      body: cacheWriteHelp,
                    }
                  : undefined
              }
            />
            <SubMetric
              label={t("usage.cacheReadTokens")}
              value={placeholder ?? tokens(cacheRead)}
              title={exact(fmtInt(cacheRead, locale))}
            />
          </div>
        </div>
      </div>

      {/* 展开按钮压在面板分隔线正中（24 见方的圆，上下各占 12）：上面的指标格底部留 14、下面的图表顶部留 20，都盖得住它；展开时子指标行不另留。
          只有箭头，名字放 aria-label 和悬停提示 */}
      <HoverTip content={t("usage.metrics.more")}>
        <button
          type="button"
          aria-label={t("usage.metrics.more")}
          aria-expanded={moreOpen}
          aria-controls={moreId}
          onClick={() => setMoreOpen((open) => !open)}
          className="absolute bottom-0 left-1/2 inline-flex h-6 w-6 -translate-x-1/2 translate-y-1/2 items-center justify-center rounded-full border border-border bg-surface text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <ChevronDown
            aria-hidden="true"
            className={cn(
              "h-3.5 w-3.5 transition-transform duration-200 motion-reduce:transition-none",
              moreOpen && "rotate-180",
            )}
          />
        </button>
      </HoverTip>
    </section>
  );
}
