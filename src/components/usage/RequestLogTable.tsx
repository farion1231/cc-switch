import { Fragment, useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useRequestLogs } from "@/lib/query/usage";
import { TablePagination } from "./TablePagination";
import { HelpTip } from "@/components/ui/help-tip";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { AppId } from "@/lib/api";
import {
  CACHE_INCLUSIVE_APP_TYPES,
  getFreshInputTokens,
  isUnpricedUsage,
  type LogFilters,
  type RequestLog,
  type UsageRangeSelection,
} from "@/types/usage";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  formatEstimatedTokensPerSecond,
  formatFirstTokenSeconds,
  formatOutputTokensPerSecond,
  formatTokensCompact,
  getLocaleFromLanguage,
  isSessionLogRequest,
  parseFiniteNumber,
} from "./format";
import { usageTable } from "./usageTable";
import { getUsageProviderLabel, usageProviderTitle } from "./providerLabel";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import {
  REQUEST_LOG_COLUMN_LABEL_KEYS,
  resolveRequestLogColumns,
  type RequestLogColumnId,
} from "./requestLogColumns";

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
  /** 要显示的列（按表格顺序）；不传按默认列 */
  columns?: RequestLogColumnId[];
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
  agy: "Antigravity",
};

export function appShortName(appType: string): string {
  return isKnownAppId(appType) ? APP_SHORT_NAME[appType] : appType;
}

const isSuccessStatus = (code: number) => code >= 200 && code < 300;

const DEFAULT_COLUMNS = resolveRequestLogColumns(undefined);

/**
 * 单条请求的缓存命中率，口径与用量页「缓存命中率」相同：
 * 缓存命中 ÷（新增输入 + 缓存写入 + 缓存命中）。没有可缓存的输入时返回 null。
 */
export function getRequestCacheHitRate(
  log: RequestLog,
): { rate: number; cacheable: number } | null {
  const cacheable =
    getFreshInputTokens(log) + log.cacheCreationTokens + log.cacheReadTokens;
  if (cacheable <= 0) return null;
  return { rate: log.cacheReadTokens / cacheable, cacheable };
}

/** 「87.3%」；只有真的全命中才写「100%」，99.96% 不进成 100 */
export function formatHitPercent(rate: number): string {
  const percent = Math.max(0, Math.min(100, rate * 100));
  if (percent >= 100) return "100%";
  return `${(Math.floor(percent * 10) / 10).toFixed(1)}%`;
}

function StatusBadge({
  code,
  title,
  className,
}: {
  code: number;
  title?: string;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "rounded-[4px] bg-danger-soft px-1 text-badge text-danger-text",
        className,
      )}
      title={title || undefined}
    >
      {code}
    </span>
  );
}

export function RequestLogTable({
  range,
  appType: dashboardAppType,
  providerName,
  model,
  statusCode,
  refreshIntervalMs,
  onOpenDetail,
  columns = DEFAULT_COLUMNS,
}: RequestLogTableProps) {
  const { t, i18n } = useTranslation();
  const { money, usdHint } = useMoneyFormat();
  const [page, setPage] = useState(0);
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

  // 带说明的列在表头挂「?」，其余只写列名
  const headerHelp: Partial<
    Record<RequestLogColumnId, { title: string; body: string }>
  > = {
    hitRate: {
      title: t("usage.metrics.hitRateHelpTitle"),
      body: t("usage.metrics.hitRateHelp"),
    },
    speed: { title: t("usage.speedHelpTitle"), body: t("usage.speedHelp") },
    firstToken: {
      title: t("usage.firstTokenHelpTitle"),
      body: t("usage.firstTokenHelp"),
    },
  };
  const renderHeader = (id: RequestLogColumnId) => {
    const label = t(REQUEST_LOG_COLUMN_LABEL_KEYS[id]);
    const help = headerHelp[id];
    if (!help) return label;
    return (
      <span className="inline-flex items-center gap-0.5">
        {label}
        <HelpTip title={help.title} align="end">
          {help.body}
        </HelpTip>
      </span>
    );
  };
  // 打开了「状态码」列就不在时间旁边再挂一个错误码
  const showStatusInTime = !columns.includes("statusCode");

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
    const firstTokenText = formatFirstTokenSeconds(firstToken);
    // 会话日志的耗时和首字都是按时间戳估的，带 ≈
    const isSession = isSessionLogRequest(log);
    const timing =
      latency != null && latency > 0
        ? {
            duration: (latency / 1000).toFixed(1),
            ttft: firstToken != null ? (firstToken / 1000).toFixed(1) : null,
          }
        : null;
    const timingTip =
      timing == null
        ? undefined
        : !isSession
          ? timing.ttft != null
            ? t("usage.timingTip", timing)
            : undefined
          : timing.ttft != null
            ? t("usage.estimatedTimingTipWithFirstToken", timing)
            : estimatedTps != null
              ? t("usage.estimatedTimingTip", timing)
              : undefined;
    const multiplier = parseFiniteNumber(log.costMultiplier);
    const modelTitle =
      log.requestModel && log.requestModel !== log.model
        ? `${log.requestModel} → ${log.model}`
        : log.model;
    const hasCache = log.cacheReadTokens > 0;
    // OpenAI 协议不单报缓存写入，这几个应用的 0 是「不知道」，不是「没写」
    const cacheWriteUnknown = CACHE_INCLUSIVE_APP_TYPES.has(log.appType);
    const hitRate = getRequestCacheHitRate(log);
    const success = isSuccessStatus(log.statusCode);

    const cells: Record<RequestLogColumnId, ReactNode> = {
      // 时间、应用两列收紧到内容宽度（w-px），多出来的宽度留给后面的数值列
      time: (
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
          {showStatusInTime && !success && (
            <StatusBadge
              className="ms-1.5"
              code={log.statusCode}
              title={log.errorMessage}
            />
          )}
        </td>
      ),
      app: (
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
      ),
      // 供应商、模型两列按比例取宽（max-w-0 让百分比宽度生效、内容截断）；
      // 比例合计 32%：40% 时常见窗口下数值列就只剩内容宽度，左松右挤。模型名通常比供应商名长，
      // 放不下的截断、悬停看全名
      provider: (
        <td className={cn(usageTable.td, "w-[14%] max-w-0")}>
          <span
            className="block truncate"
            title={usageProviderTitle(providerLabel)}
          >
            {provider}
          </span>
        </td>
      ),
      model: (
        <td className={cn(usageTable.td, usageTable.mono, "w-[18%] max-w-0")}>
          <span className="block truncate" title={modelTitle}>
            {log.model}
          </span>
        </td>
      ),
      freshInput: (
        <td
          className={usageTable.td}
          title={
            isCacheInclusive
              ? `${fmtInt(freshInput, locale)} (${t("usage.rawInputLabel")}: ${fmtInt(log.inputTokens, locale)})`
              : fmtInt(freshInput, locale)
          }
        >
          {formatTokensCompact(freshInput, locale)}
        </td>
      ),
      output: (
        <td className={usageTable.td} title={fmtInt(log.outputTokens, locale)}>
          {formatTokensCompact(log.outputTokens, locale)}
        </td>
      ),
      cacheRead: (
        <td
          className={cn(usageTable.td, !hasCache && usageTable.muted)}
          title={t("usage.cacheTip", {
            read: fmtInt(log.cacheReadTokens, locale),
            write: fmtInt(log.cacheCreationTokens, locale),
          })}
        >
          {hasCache ? formatTokensCompact(log.cacheReadTokens, locale) : "—"}
        </td>
      ),
      cacheWrite: (
        <td
          className={cn(
            usageTable.td,
            (cacheWriteUnknown || log.cacheCreationTokens <= 0) &&
              usageTable.muted,
          )}
          title={
            cacheWriteUnknown
              ? t("usage.cacheWriteNotReported")
              : fmtInt(log.cacheCreationTokens, locale)
          }
        >
          {!cacheWriteUnknown && log.cacheCreationTokens > 0
            ? formatTokensCompact(log.cacheCreationTokens, locale)
            : "—"}
        </td>
      ),
      hitRate: (
        <td
          className={cn(usageTable.td, hitRate == null && usageTable.muted)}
          title={
            hitRate == null
              ? undefined
              : t("usage.logColumns.hitRateTip", {
                  read: fmtInt(log.cacheReadTokens, locale),
                  total: fmtInt(hitRate.cacheable, locale),
                })
          }
        >
          {hitRate == null ? "—" : formatHitPercent(hitRate.rate)}
        </td>
      ),
      cost: (
        <td
          className={cn(
            usageTable.td,
            "font-medium",
            unpriced && "font-normal text-fg-3",
          )}
          title={
            [
              unpriced ? undefined : usdHint(log.totalCostUsd),
              multiplier != null && multiplier !== 1
                ? `${t("usage.costMultiplier")} ×${multiplier.toFixed(2)}`
                : undefined,
            ]
              .filter(Boolean)
              .join(" · ") || undefined
          }
        >
          {unpriced ? t("usage.unpriced") : money(log.totalCostUsd, 4)}
        </td>
      ),
      speed: (
        <td
          className={cn(usageTable.td, tps == null && usageTable.muted)}
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
      ),
      firstToken: (
        <td
          className={cn(
            usageTable.td,
            firstTokenText == null && usageTable.muted,
          )}
          title={timingTip}
        >
          {isSession && firstTokenText != null && "≈"}
          {firstTokenText ?? "—"}
        </td>
      ),
      duration: (
        <td
          className={cn(usageTable.td, timing == null && usageTable.muted)}
          title={
            timing == null
              ? undefined
              : `${isSession ? "≈" : ""}${fmtInt(latency, locale)} ms`
          }
        >
          {timing == null ? "—" : `${isSession ? "≈" : ""}${timing.duration}s`}
        </td>
      ),
      statusCode: (
        <td className={usageTable.td}>
          {success ? (
            log.statusCode
          ) : (
            <StatusBadge code={log.statusCode} title={log.errorMessage} />
          )}
        </td>
      ),
    };

    return (
      <tr
        key={log.requestId}
        className={onOpenDetail ? usageTable.rowInteractive : usageTable.row}
        onClick={() => onOpenDetail?.(log.requestId)}
      >
        {columns.map((id) => (
          <Fragment key={id}>{cells[id]}</Fragment>
        ))}
      </tr>
    );
  };

  // 默认 10 列时 620px：最小窗口（900）展开侧栏表格区只有 644px，再宽最右的速度列就被挤进横向滚动；
  // 多开一列加一份宽度，列再多就横向滚动，不把每列挤到看不清
  const minWidth = 620 + Math.max(0, columns.length - 10) * 64;

  return (
    <div className="flex flex-col">
      <div className={usageTable.scroller}>
        <table
          className={usageTable.table}
          style={{ minWidth }}
          aria-label={t("usage.requestLogs")}
        >
          <thead>
            <tr className={usageTable.headRow}>
              {columns.map((id) => (
                <th key={id} className={usageTable.th}>
                  {renderHeader(id)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {logs.length === 0 ? (
              <tr>
                <td colSpan={columns.length} className={usageTable.empty}>
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
