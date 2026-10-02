import { useEffect, useState, type ReactNode } from "react";
import { RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import {
  formatRelativeTime,
  pickLines,
  type QuotaLine,
  type QuotaTone,
} from "./quotaRules";

const TONE_TEXT: Record<QuotaTone, string> = {
  normal: "text-fg-2",
  muted: "text-fg-3",
  warning: "font-medium text-warning-text",
  danger: "font-medium text-danger-text",
};

const TONE_FILL: Record<QuotaTone, string> = {
  normal: "bg-chart-1",
  muted: "bg-chart-1",
  warning: "bg-warning",
  danger: "bg-danger",
};

/** 每 30 秒刷新一次「x 分钟前」 */
function useNow(active: boolean) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!active) return;
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  }, [active]);
  return now;
}

interface QuotaLinesProps {
  lines: QuotaLine[];
  max?: number;
  queriedAt?: number | null;
  loading?: boolean;
  onRefresh?: () => void;
}

/**
 * 卡片右侧的额度列（v7）：最多两行、右对齐、平时灰色。点一下重查，悬停说明更新时间和重置时间。
 */
export function QuotaLines({
  lines,
  max = 2,
  queriedAt,
  loading = false,
  onRefresh,
}: QuotaLinesProps) {
  const { t } = useTranslation();
  const now = useNow(Boolean(queriedAt));
  const shown = pickLines(lines, max);
  if (shown.length === 0) return null;

  const title = [
    ...lines.map((line) => line.detail ?? line.text),
    queriedAt
      ? t("quota.updatedAt", { time: formatRelativeTime(queriedAt, now, t) })
      : null,
    onRefresh ? t("quota.clickToRefresh") : null,
  ]
    .filter(Boolean)
    .join("\n");

  const body = shown.map((line) => (
    <span
      key={line.key}
      className={cn("max-w-full truncate", TONE_TEXT[line.tone])}
    >
      {line.text}
    </span>
  ));

  const className = cn(
    "flex w-[136px] shrink-0 flex-col items-end text-caption leading-[18px] tabular-nums whitespace-nowrap",
    loading && "opacity-60",
  );

  if (!onRefresh) {
    return (
      <span className={className} title={title}>
        {body}
      </span>
    );
  }
  return (
    <button
      type="button"
      className={cn(
        className,
        "rounded-control text-end transition-opacity focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
      )}
      title={title}
      aria-busy={loading}
      disabled={loading}
      onClick={(event) => {
        event.stopPropagation();
        onRefresh();
      }}
    >
      {body}
    </button>
  );
}

interface QuotaBarsProps {
  /** 每档的名字和那一行（名字单独一列） */
  rows: { label: string; line: QuotaLine; note?: string }[];
  title?: ReactNode;
  queriedAt?: number | null;
  loading?: boolean;
  onRefresh?: () => void;
  footer?: ReactNode;
  className?: string;
}

/** 展开的额度条（授权中心、多套餐展开）：条越短剩得越少 */
export function QuotaBars({
  rows,
  title,
  queriedAt,
  loading = false,
  onRefresh,
  footer,
  className,
}: QuotaBarsProps) {
  const { t } = useTranslation();
  const now = useNow(Boolean(queriedAt));

  return (
    <div
      className={cn(
        "rounded-panel border border-border bg-surface px-3.5 py-2.5",
        className,
      )}
    >
      {(title || queriedAt || onRefresh) && (
        <div className="mb-1.5 flex items-center justify-between gap-2 text-caption">
          <span className="font-medium text-fg-2">{title}</span>
          <span className="flex items-center gap-1 text-fg-3">
            {queriedAt
              ? t("quota.updatedAt", {
                  time: formatRelativeTime(queriedAt, now, t),
                })
              : null}
            {onRefresh && (
              <button
                type="button"
                onClick={onRefresh}
                disabled={loading}
                className="rounded-control p-1 text-fg-3 hover:bg-subtle hover:text-fg-1 disabled:opacity-50"
                aria-label={t("subscription.refresh")}
                title={t("subscription.refresh")}
              >
                <RefreshCw
                  className={cn("h-3 w-3", loading && "animate-spin")}
                />
              </button>
            )}
          </span>
        </div>
      )}
      <div className="flex flex-col gap-1">
        {rows.map(({ label, line, note }) => {
          const width = Number.isFinite(line.left)
            ? Math.max(0, Math.min(100, line.left))
            : 100;
          return (
            <div
              key={line.key}
              className="flex h-[18px] items-center gap-2 text-caption"
              title={line.detail}
            >
              <span className="w-[72px] shrink-0 truncate text-fg-2">
                {label}
              </span>
              <span
                role="meter"
                aria-label={`${label}: ${line.value ?? line.text}`}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={Math.round(width)}
                className="relative h-1 w-[120px] shrink-0 overflow-hidden rounded-full bg-chart-grid"
              >
                <span
                  className={cn(
                    "absolute inset-y-0 start-0 rounded-full",
                    TONE_FILL[line.tone],
                  )}
                  style={{ width: `${width}%` }}
                />
              </span>
              <span
                className={cn(
                  "shrink-0 text-end tabular-nums whitespace-nowrap",
                  line.tone === "normal" ? "text-fg-1" : TONE_TEXT[line.tone],
                )}
              >
                {line.value ?? line.text}
              </span>
              {note && (
                <span className="min-w-0 truncate text-fg-3">{note}</span>
              )}
            </div>
          );
        })}
      </div>
      {footer}
    </div>
  );
}
