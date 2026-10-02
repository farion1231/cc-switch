import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { useRequestLogs } from "@/lib/query/usage";
import { HelpTip } from "@/components/ui/help-tip";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { AppId } from "@/lib/api";
import {
  getFreshInputTokens,
  isUnpricedUsage,
  type LogFilters,
  type RequestLog,
  type UsageRangeSelection,
} from "@/types/usage";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  fmtUsd,
  formatOutputTokensPerSecond,
  formatTokensCompact,
  getLocaleFromLanguage,
  parseFiniteNumber,
} from "./format";
import { usageTable } from "./usageTable";

interface RequestLogTableProps {
  range: UsageRangeSelection;
  /** 旧接口保留；时间范围由页面顶部的筛选统一控制 */
  rangeLabel?: string;
  appType?: string;
  providerName?: string;
  model?: string;
  /** 状态码筛选（页签行右侧的下拉） */
  statusCode?: number;
  refreshIntervalMs: number;
  /** 点一行打开请求详情 */
  onOpenDetail?: (requestId: string) => void;
}

const pad2 = (value: number) => String(value).padStart(2, "0");

/** 「09-30 14:21」：表格里只要月日时分，完整时间在详情抽屉里。 */
export function formatLogTime(createdAt: number): string {
  const date = new Date(createdAt * 1000);
  return `${pad2(date.getMonth() + 1)}-${pad2(date.getDate())} ${pad2(
    date.getHours(),
  )}:${pad2(date.getMinutes())}`;
}

export function isKnownAppId(appType: string): appType is AppId {
  return appType in APP_DISPLAY_NAME;
}

export function appDisplayName(appType: string): string {
  return isKnownAppId(appType) ? APP_DISPLAY_NAME[appType] : appType;
}

const isSuccessStatus = (code: number) => code >= 200 && code < 300;

export function RequestLogTable({
  range,
  appType: dashboardAppType,
  providerName,
  model,
  statusCode,
  refreshIntervalMs,
  onOpenDetail,
}: RequestLogTableProps) {
  const { t, i18n } = useTranslation();
  const [page, setPage] = useState(0);
  const [pageDraft, setPageDraft] = useState<string | null>(null);
  const pageSize = 20;

  const effectiveFilters: LogFilters = {
    appType:
      dashboardAppType && dashboardAppType !== "all"
        ? dashboardAppType
        : undefined,
    providerName,
    model,
    statusCode,
  };

  const { data: result, isLoading } = useRequestLogs({
    filters: effectiveFilters,
    range,
    page,
    pageSize,
    options: {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  });

  const logs = result?.data ?? [];
  const total = result?.total ?? 0;
  const totalPages = Math.max(1, Math.ceil(total / pageSize));

  useEffect(() => {
    setPage(0);
    setPageDraft(null);
  }, [
    dashboardAppType,
    providerName,
    model,
    statusCode,
    range.customEndDate,
    range.customStartDate,
    range.preset,
  ]);

  const commitPageDraft = () => {
    if (pageDraft == null) return;
    const trimmed = pageDraft.trim();
    setPageDraft(null);
    if (!/^\d+$/.test(trimmed)) return;
    const parsed = Number(trimmed);
    if (parsed < 1 || parsed > totalPages) return;
    setPage(parsed - 1);
  };

  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);

  if (isLoading) {
    return <div className={usageTable.skeleton} />;
  }

  const renderRow = (log: RequestLog) => {
    const unpriced = isUnpricedUsage(log);
    const freshInput = getFreshInputTokens(log);
    const isCacheInclusive = log.inputTokens !== freshInput;
    const time = formatLogTime(log.createdAt);
    const provider = log.providerName || t("usage.unknownProvider");
    const tps = formatOutputTokensPerSecond(log);
    const latency = parseFiniteNumber(log.latencyMs);
    const firstToken = parseFiniteNumber(log.firstTokenMs);
    const timingTip =
      latency != null && latency > 0 && firstToken != null
        ? t("usage.timingTip", {
            duration: (latency / 1000).toFixed(1),
            ttft: (firstToken / 1000).toFixed(1),
          })
        : undefined;
    const multiplier = parseFiniteNumber(log.costMultiplier);
    const modelTitle =
      log.requestModel && log.requestModel !== log.model
        ? `${log.requestModel} → ${log.model}`
        : log.model;
    const hasCache = log.cacheReadTokens > 0;

    return (
      <tr
        key={log.requestId}
        className={onOpenDetail ? usageTable.rowInteractive : usageTable.row}
        onClick={() => onOpenDetail?.(log.requestId)}
      >
        <td className={usageTable.td}>
          <button
            type="button"
            className="rounded-[4px] text-start tabular-nums focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            aria-label={t("usage.openRequestDetail", { time, provider })}
            onClick={(event) => {
              event.stopPropagation();
              onOpenDetail?.(log.requestId);
            }}
          >
            {time}
          </button>
          {!isSuccessStatus(log.statusCode) && (
            <span
              className="ms-1.5 rounded-[4px] bg-danger-soft px-1 text-badge text-danger-text"
              title={log.errorMessage || undefined}
            >
              {log.statusCode}
            </span>
          )}
        </td>
        <td className={usageTable.td}>
          <span
            className="flex max-w-[96px] items-center gap-1.5"
            title={appDisplayName(log.appType)}
          >
            {isKnownAppId(log.appType) && (
              <AppGlyph
                app={log.appType}
                size={14}
                badgeClassName="bg-surface"
              />
            )}
            <span className="truncate">{appDisplayName(log.appType)}</span>
          </span>
        </td>
        <td className={usageTable.td}>
          <span className="block max-w-[104px] truncate" title={provider}>
            {provider}
          </span>
        </td>
        <td className={cn(usageTable.td, usageTable.mono)}>
          <span className="block max-w-[108px] truncate" title={modelTitle}>
            {log.model}
          </span>
        </td>
        <td
          className={usageTable.tdEnd}
          title={
            isCacheInclusive
              ? `${fmtInt(freshInput, locale)} (${t("usage.rawInputLabel")}: ${fmtInt(log.inputTokens, locale)})`
              : fmtInt(freshInput, locale)
          }
        >
          {formatTokensCompact(freshInput, locale)}
        </td>
        <td
          className={usageTable.tdEnd}
          title={fmtInt(log.outputTokens, locale)}
        >
          {formatTokensCompact(log.outputTokens, locale)}
        </td>
        <td
          className={cn(usageTable.tdEnd, !hasCache && usageTable.muted)}
          title={t("usage.cacheTip", {
            read: fmtInt(log.cacheReadTokens, locale),
            write: fmtInt(log.cacheCreationTokens, locale),
          })}
        >
          {hasCache ? formatTokensCompact(log.cacheReadTokens, locale) : "—"}
        </td>
        <td
          className={cn(
            usageTable.tdEnd,
            "font-medium",
            unpriced && "font-normal text-fg-3",
          )}
          title={
            multiplier != null && multiplier !== 1
              ? `${t("usage.costMultiplier")} ×${multiplier.toFixed(2)}`
              : undefined
          }
        >
          {unpriced ? t("usage.unpriced") : fmtUsd(log.totalCostUsd, 4)}
        </td>
        <td
          className={cn(usageTable.tdEnd, tps == null && usageTable.muted)}
          title={timingTip}
        >
          {tps == null ? (
            "—"
          ) : (
            <>
              {tps}
              <span className="ms-0.5 text-badge font-normal text-fg-3">
                tok/s
              </span>
            </>
          )}
        </td>
      </tr>
    );
  };

  return (
    <div className="flex flex-col">
      <div className={usageTable.scroller}>
        <table
          className={cn(usageTable.table, "min-w-[700px]")}
          aria-label={t("usage.requestLogs")}
        >
          <thead>
            <tr className={usageTable.headRow}>
              <th className={usageTable.th}>{t("usage.time")}</th>
              <th className={usageTable.th}>{t("usage.app")}</th>
              <th className={usageTable.th}>{t("usage.provider")}</th>
              <th className={usageTable.th}>{t("usage.model")}</th>
              <th className={usageTable.thEnd}>{t("usage.freshInput")}</th>
              <th className={usageTable.thEnd}>{t("usage.outputTokens")}</th>
              <th className={usageTable.thEnd}>{t("usage.cacheReadTokens")}</th>
              <th className={usageTable.thEnd}>{t("usage.cost")}</th>
              <th className={usageTable.thEnd}>
                <span className="inline-flex items-center gap-0.5">
                  {t("usage.speed")}
                  <HelpTip title={t("usage.speedHelpTitle")} align="end">
                    {t("usage.speedHelp")}
                  </HelpTip>
                </span>
              </th>
            </tr>
          </thead>
          <tbody>
            {logs.length === 0 ? (
              <tr>
                <td colSpan={9} className={usageTable.empty}>
                  {t("usage.noData")}
                </td>
              </tr>
            ) : (
              logs.map(renderRow)
            )}
          </tbody>
        </table>
      </div>

      <div className="flex h-10 items-center gap-2 text-caption text-fg-3">
        <span className="tabular-nums">
          {t("usage.totalRecords", { total })}
        </span>
        <div className="flex-1" />
        <button
          type="button"
          className="inline-flex h-7 w-7 items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1 disabled:pointer-events-none disabled:opacity-45"
          aria-label={t("usage.prevPage")}
          title={t("usage.prevPage")}
          disabled={page === 0}
          onClick={() => setPage((p) => Math.max(0, p - 1))}
        >
          <ChevronLeft className="h-4 w-4" />
        </button>
        <span className="flex items-center gap-1 tabular-nums text-fg-2">
          <input
            type="text"
            inputMode="numeric"
            aria-label={t("usage.pageInputPlaceholder")}
            className="h-6 w-9 rounded-[4px] border border-transparent bg-transparent text-center text-caption text-fg-1 hover:border-border focus:border-border-strong focus:outline-none"
            value={pageDraft ?? String(page + 1)}
            onChange={(event) => setPageDraft(event.target.value)}
            onFocus={(event) => event.target.select()}
            onBlur={commitPageDraft}
            onKeyDown={(event) => {
              if (event.key === "Enter") commitPageDraft();
              if (event.key === "Escape") setPageDraft(null);
            }}
          />
          <span>/ {fmtInt(totalPages, locale)}</span>
        </span>
        <button
          type="button"
          className="inline-flex h-7 w-7 items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1 disabled:pointer-events-none disabled:opacity-45"
          aria-label={t("usage.nextPage")}
          title={t("usage.nextPage")}
          disabled={page >= totalPages - 1}
          onClick={() => setPage((p) => Math.min(totalPages - 1, p + 1))}
        >
          <ChevronRight className="h-4 w-4" />
        </button>
      </div>
    </div>
  );
}
