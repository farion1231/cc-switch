import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useRequestLogs } from "@/lib/query/usage";
import { TablePagination } from "./TablePagination";
import { HelpTip } from "@/components/ui/help-tip";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { AppId } from "@/lib/api";
import type { ColumnVisibility } from "@/types/table";
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
  formatEstimatedTokensPerSecond,
  formatOutputTokensPerSecond,
  formatTokensCompact,
  getLocaleFromLanguage,
  parseFiniteNumber,
} from "./format";
import { usageTable } from "./usageTable";
import { getUsageProviderLabel, usageProviderTitle } from "./providerLabel";
import {
  getColumnLayout,
  REQUEST_LOG_COLUMNS,
  type RequestLogColumnId,
} from "./tableColumns";

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
  columnVisibility?: ColumnVisibility<RequestLogColumnId>;
}

const pad2 = (value: number) => String(value).padStart(2, "0");

const isSameLocalDay = (a: Date, b: Date) =>
  a.getFullYear() === b.getFullYear() &&
  a.getMonth() === b.getMonth() &&
  a.getDate() === b.getDate();

/**
 * 表格里的时间：今天（本地时区）只显示时刻「14:32:05」，
 * 别的日子显示「09-30 14:21」。完整时间放在悬停提示和详情抽屉里。
 */
export function formatLogTime(createdAt: number, now = new Date()): string {
  const date = new Date(createdAt * 1000);
  const clock = `${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
  if (isSameLocalDay(date, now)) {
    return `${clock}:${pad2(date.getSeconds())}`;
  }
  return `${pad2(date.getMonth() + 1)}-${pad2(date.getDate())} ${clock}`;
}

/** 「2026-09-30 14:21:05」：悬停时看完整的本地时间。 */
export function formatLogFullTime(createdAt: number): string {
  const date = new Date(createdAt * 1000);
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(
    date.getDate(),
  )} ${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(
    date.getSeconds(),
  )}`;
}

export function isKnownAppId(appType: string): appType is AppId {
  return appType in APP_DISPLAY_NAME;
}

export function appDisplayName(appType: string): string {
  return isKnownAppId(appType) ? APP_DISPLAY_NAME[appType] : appType;
}

/**
 * 请求日志「应用」列用的短名：图标已经区分了品牌（Claude Code / Desktop 靠角标），
 * 列里只留最短能认出的名字，把宽度让给供应商列。全名在悬停提示里。
 */
const APP_SHORT_NAME: Record<AppId, string> = {
  claude: "Claude",
  "claude-desktop": "Desktop",
  codex: "Codex",
  gemini: "Gemini",
  grokbuild: "Grok",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
  pi: "Pi",
  mcode: "MiniMax",
};

export function appShortName(appType: string): string {
  return isKnownAppId(appType) ? APP_SHORT_NAME[appType] : appType;
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
  columnVisibility,
}: RequestLogTableProps) {
  const { t, i18n } = useTranslation();
  const [page, setPage] = useState(0);
  const pageSize = 20;
  const layout = getColumnLayout(REQUEST_LOG_COLUMNS, columnVisibility);
  const { isVisible } = layout;

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
  }, [
    dashboardAppType,
    providerName,
    model,
    statusCode,
    range.customEndDate,
    range.customStartDate,
    range.preset,
  ]);

  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);
  // 「今天」按本地时区判断；每次渲染（含自动刷新）取一次当前时间
  const now = new Date();

  if (isLoading) {
    return <div className={usageTable.skeleton} />;
  }

  const renderRow = (log: RequestLog) => {
    const unpriced = isUnpricedUsage(log);
    const freshInput = getFreshInputTokens(log);
    const isCacheInclusive = log.inputTokens !== freshInput;
    const time = formatLogTime(log.createdAt, now);
    const fullTime = formatLogFullTime(log.createdAt);
    const providerLabel = getUsageProviderLabel(log.providerName, t);
    const provider = providerLabel.shortLabel;
    const exactTps = formatOutputTokensPerSecond(log);
    // 会话日志导入的请求没有首字计时，速度是按日志时间戳估的，前面带 ≈
    const estimatedTps =
      exactTps == null ? formatEstimatedTokensPerSecond(log) : null;
    const tps = exactTps ?? estimatedTps;
    const latency = parseFiniteNumber(log.latencyMs);
    const firstToken = parseFiniteNumber(log.firstTokenMs);
    const hasFirstToken = firstToken != null && firstToken >= 0;
    const timingTip =
      latency != null && latency > 0 && firstToken != null
        ? t("usage.timingTip", {
            duration: (latency / 1000).toFixed(1),
            ttft: (firstToken / 1000).toFixed(1),
          })
        : estimatedTps != null && latency != null
          ? t("usage.estimatedTimingTip", {
              duration: (latency / 1000).toFixed(1),
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
        {isVisible("time") && (
          <td className={cn(usageTable.td, "w-px")}>
            <button
              type="button"
              className="rounded-[4px] text-start tabular-nums focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              title={fullTime}
              aria-label={t("usage.openRequestDetail", {
                time: fullTime,
                provider,
              })}
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
        )}
        {isVisible("app") && (
          <td className={cn(usageTable.td, "w-px")}>
            <span
              className="flex max-w-[88px] items-center gap-1.5"
              title={appDisplayName(log.appType)}
            >
              {isKnownAppId(log.appType) && (
                <AppGlyph
                  app={log.appType}
                  size={14}
                  badgeClassName="bg-surface"
                />
              )}
              <span className="truncate" aria-hidden="true">
                {appShortName(log.appType)}
              </span>
              <span className="sr-only">{appDisplayName(log.appType)}</span>
            </span>
          </td>
        )}
        {isVisible("provider") && (
          <td className={cn(usageTable.td, "max-w-0")}>
            <span
              className="block truncate"
              title={usageProviderTitle(providerLabel)}
            >
              {provider}
            </span>
          </td>
        )}
        {isVisible("model") && (
          <td className={cn(usageTable.td, usageTable.mono, "max-w-0")}>
            <span className="block truncate" title={modelTitle}>
              {log.model}
            </span>
          </td>
        )}
        {isVisible("freshInput") && (
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
        )}
        {isVisible("outputTokens") && (
          <td
            className={usageTable.tdEnd}
            title={fmtInt(log.outputTokens, locale)}
          >
            {formatTokensCompact(log.outputTokens, locale)}
          </td>
        )}
        {isVisible("cacheReadTokens") && (
          <td
            className={cn(usageTable.tdEnd, !hasCache && usageTable.muted)}
            title={t("usage.cacheTip", {
              read: fmtInt(log.cacheReadTokens, locale),
              write: fmtInt(log.cacheCreationTokens, locale),
            })}
          >
            {hasCache ? formatTokensCompact(log.cacheReadTokens, locale) : "—"}
          </td>
        )}
        {isVisible("cost") && (
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
        )}
        {isVisible("speed") && (
          <td
            className={cn(usageTable.tdEnd, tps == null && usageTable.muted)}
            title={timingTip}
          >
            {tps == null ? (
              "—"
            ) : (
              <>
                {estimatedTps != null && "≈"}
                {tps}
                <span className="ms-0.5 text-badge font-normal text-fg-3">
                  tok/s
                </span>
              </>
            )}
          </td>
        )}
        {isVisible("firstToken") && (
          <td
            className={cn(usageTable.tdEnd, !hasFirstToken && usageTable.muted)}
            title={
              hasFirstToken
                ? t("usage.firstTokenMilliseconds", {
                    value: firstToken.toLocaleString(locale, {
                      maximumFractionDigits: 20,
                    }),
                  })
                : undefined
            }
          >
            {hasFirstToken ? `${(firstToken / 1000).toFixed(1)}s` : "—"}
          </td>
        )}
      </tr>
    );
  };

  return (
    <div className="flex flex-col">
      <div className={cn(usageTable.scroller, "relative")}>
        <table
          className={usageTable.table}
          style={{ minWidth: layout.minWidth }}
          aria-label={t("usage.requestLogs")}
        >
          {/* 列宽作为起点，自动布局为多语言表头和日期留足空间。 */}
          <colgroup>
            {REQUEST_LOG_COLUMNS.filter(({ id }) => isVisible(id)).map(
              (column) => (
                <col key={column.id} style={{ width: column.minWidth }} />
              ),
            )}
          </colgroup>
          <thead>
            <tr className={usageTable.headRow}>
              {isVisible("time") && (
                <th className={usageTable.th}>{t("usage.time")}</th>
              )}
              {isVisible("app") && (
                <th className={usageTable.th}>{t("usage.app")}</th>
              )}
              {isVisible("provider") && (
                <th className={usageTable.th}>{t("usage.provider")}</th>
              )}
              {isVisible("model") && (
                <th className={usageTable.th}>{t("usage.model")}</th>
              )}
              {isVisible("freshInput") && (
                <th className={usageTable.thEnd}>{t("usage.freshInput")}</th>
              )}
              {isVisible("outputTokens") && (
                <th className={usageTable.thEnd}>{t("usage.outputTokens")}</th>
              )}
              {isVisible("cacheReadTokens") && (
                <th className={usageTable.thEnd}>
                  {t("usage.cacheReadTokens")}
                </th>
              )}
              {isVisible("cost") && (
                <th className={usageTable.thEnd}>{t("usage.cost")}</th>
              )}
              {isVisible("speed") && (
                <th className={usageTable.thEnd}>
                  <span className="inline-flex items-center gap-0.5">
                    {t("usage.speed")}
                    <HelpTip title={t("usage.speedHelpTitle")} align="end">
                      {t("usage.speedHelp")}
                    </HelpTip>
                  </span>
                </th>
              )}
              {isVisible("firstToken") && (
                <th className={usageTable.thEnd}>{t("usage.firstToken")}</th>
              )}
            </tr>
          </thead>
          <tbody>
            {logs.length === 0 ? (
              <tr>
                <td colSpan={layout.visibleCount} className={usageTable.empty}>
                  {t("usage.noData")}
                </td>
              </tr>
            ) : (
              logs.map(renderRow)
            )}
          </tbody>
        </table>
      </div>

      <TablePagination
        page={page}
        totalPages={totalPages}
        total={total}
        onPageChange={setPage}
      />
    </div>
  );
}
