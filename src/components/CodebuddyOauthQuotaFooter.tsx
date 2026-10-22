import React from "react";
import { RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ProviderMeta } from "@/types";
import type { CodebuddyQuota } from "@/types/subscription";
import { useCodebuddyOauthQuota } from "@/lib/query/subscription";
import { ResetSlot, useNow } from "@/components/quota/QuotaLines";
import {
  countdownStr,
  formatRelativeTime,
} from "@/components/quota/quotaRules";
import { cn } from "@/lib/utils";

interface CodebuddyOauthQuotaFooterProps {
  meta?: ProviderMeta;
  inline?: boolean;
  /** 是否为当前激活的供应商 */
  isCurrent?: boolean;
}

/**
 * CodeBuddy 的上游周期结束时间可能是 `YYYY-MM-DD HH:mm:ss`（本地时间）或毫秒
 * 时间戳字符串；配额展示层需要 ISO 时间。
 */
function cycleEndIso(value: string | null | undefined): string | null {
  const raw = value?.trim();
  if (!raw) return null;
  if (/^\d+$/.test(raw)) {
    const ms = Number(raw);
    if (!Number.isFinite(ms)) return null;
    const date = new Date(ms);
    return Number.isNaN(date.getTime()) ? null : date.toISOString();
  }
  const parsed = Date.parse(raw.replace(" ", "T"));
  return Number.isFinite(parsed) ? new Date(parsed).toISOString() : null;
}

/** 多个资源包时取最早到期的周期结束时间（ISO）。 */
export function earliestCycleEnd(
  packages: CodebuddyQuota["packages"],
): string | null {
  const times = packages
    .map((packageInfo) => cycleEndIso(packageInfo.cycle_end))
    .filter((value): value is string => Boolean(value))
    .map((value) => Date.parse(value))
    .filter((value) => Number.isFinite(value));
  if (times.length === 0) return null;
  return new Date(Math.min(...times)).toISOString();
}

/**
 * 按界面语言格式化 credits：整数不显示小数，有小数时最多两位（企业额度会带小数）。
 */
export function formatCodebuddyCredits(
  value: number,
  locale: string | undefined,
): string {
  if (!Number.isFinite(value)) return "-";
  try {
    return new Intl.NumberFormat(locale, {
      maximumFractionDigits: Number.isInteger(value) ? 0 : 2,
    }).format(value);
  } catch {
    return String(value);
  }
}

/**
 * CodeBuddy 供应商卡片的额度 footer。
 *
 * 数据源是 cc-switch 自管的 CodeBuddy OAuth 账号（按供应商的 authBinding 解析，
 * 未绑定账号时跟随默认账号）。通用 SubscriptionQuotaView 只渲染固定的几档订阅
 * 窗口且只有百分比，无法表达 credits 余额，因此这里单独渲染「剩余 / 总量」。
 */
const CodebuddyOauthQuotaFooter: React.FC<CodebuddyOauthQuotaFooterProps> = ({
  isCurrent = false,
  meta,
}) => {
  const { t, i18n } = useTranslation();
  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useCodebuddyOauthQuota(meta, { enabled: true, autoQuery: isCurrent });
  const now = useNow(Boolean(quota?.success));
  const resetAt = React.useMemo(
    () => earliestCycleEnd(quota?.packages ?? []),
    [quota],
  );
  const countdown = countdownStr(resetAt, now);
  const remaining = quota
    ? formatCodebuddyCredits(quota.remaining, i18n.language)
    : "-";
  const total = quota
    ? formatCodebuddyCredits(quota.total, i18n.language)
    : "-";
  const label = t("subscription.short.codebuddyCredits", {
    defaultValue: "额度",
  });
  const fullText = quota?.success
    ? `${t("subscription.codebuddyCredits", { defaultValue: "CodeBuddy 额度" })}: ${t(
        "subscription.codebuddyCreditsText",
        {
          remaining,
          total,
          defaultValue: `剩余 ${remaining} / ${total} credits`,
        },
      )}${countdown ? ` · ${t("subscription.resetsIn", { time: countdown })}` : ""}`
    : "";

  const refreshLabel = t("authCenter.quota.refresh", {
    defaultValue: "刷新额度",
  });
  const clickHint = t("quota.clickToRefresh", {
    defaultValue: "点击重新查询",
  });
  // 与 Codex / 订阅额度列（QuotaLines）一致：tooltip 里带「上次查询时间」。
  const updatedAt = quota?.queried_at
    ? t("quota.updatedAt", {
        time: formatRelativeTime(quota.queried_at, now, t),
      })
    : null;
  const busy = loading;

  // 刷新图标位于额度文字之前，平时 invisible 但保留宽度，hover 时文字不位移；
  // 鼠标移入或键盘聚焦整行时出现，与 Codex / 订阅额度列的交互一致。
  const refreshIcon = (
    <RefreshCw
      aria-hidden
      className={cn(
        "h-3 w-3 shrink-0 text-fg-3 transition-opacity group-hover:text-fg-2",
        busy
          ? "visible opacity-100 motion-safe:animate-spin"
          : "invisible opacity-0 group-hover:visible group-hover:opacity-100 group-focus-visible:visible group-focus-visible:opacity-100",
      )}
      strokeWidth={1.75}
    />
  );

  if (!quota) {
    if (!busy) return null;
    return (
      <span className="inline-flex items-center gap-1 text-caption text-fg-3">
        {refreshIcon}…
      </span>
    );
  }

  const content = quota.success ? (
    <>
      <span className="shrink-0 text-fg-2">{label}</span>
      <span className="min-w-0 truncate text-fg-1">
        {remaining}
        <span className="text-fg-3">/{total}</span>
      </span>
      <ResetSlot countdown={countdown} className="ms-0" />
    </>
  ) : (
    <span className="min-w-0 truncate">
      {t("quota.failed", { defaultValue: "额度没查到" })}
    </span>
  );

  return (
    <button
      type="button"
      aria-busy={busy}
      aria-disabled={busy}
      aria-label={refreshLabel}
      title={[quota.success ? fullText : quota.error, updatedAt, clickHint]
        .filter(Boolean)
        .join("\n")}
      onClick={(event) => {
        event.stopPropagation();
        if (!busy) void refetch();
      }}
      className="group inline-flex min-w-0 items-center gap-1 rounded-control text-caption tabular-nums focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
    >
      {refreshIcon}
      {content}
    </button>
  );
};

export default CodebuddyOauthQuotaFooter;
