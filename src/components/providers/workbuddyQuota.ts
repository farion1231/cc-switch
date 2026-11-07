import { useRef } from "react";
import type { TFunction } from "i18next";
import { useQuery } from "@tanstack/react-query";
import { authApi } from "@/lib/api";
import type { WorkBuddyQuota } from "@/lib/api/auth";
import {
  failedLines,
  formatRelativeTime,
  toneForLeft,
  type QuotaDisplay,
  type QuotaLine,
} from "@/components/quota/quotaRules";

export const WORKBUDDY_QUOTA_KEY = ["workbuddy-account-quota"] as const;

const REFETCH_INTERVAL = 5 * 60 * 1000;

/**
 * WorkBuddy 账号的套餐与积分。卡片、表单和授权中心共用一份缓存；后端 5 分钟内走缓存，
 * `refresh()` 跳过缓存重查（↻ 用）。只有 WorkBuddy 正在运行时当前账号才查得到新的。
 */
export function useWorkBuddyAccountQuota(
  accountId: string | null,
  options: { autoRefresh?: boolean } = {},
) {
  const force = useRef(false);
  const query = useQuery({
    queryKey: [...WORKBUDDY_QUOTA_KEY, accountId],
    queryFn: () => {
      const skipCache = force.current;
      force.current = false;
      return authApi.workbuddyAccountQuota(accountId!, skipCache);
    },
    enabled: Boolean(accountId),
    staleTime: REFETCH_INTERVAL,
    refetchInterval: options.autoRefresh ? REFETCH_INTERVAL : false,
    refetchOnWindowFocus: false,
    retry: false,
  });
  const refresh = () => {
    force.current = true;
    return query.refetch();
  };
  return { ...query, refresh };
}

/** 积分数：一万以上用紧凑写法（「12.3万」「12.3K」） */
export function formatCredits(value: number, locale: string): string {
  try {
    return new Intl.NumberFormat(
      locale,
      value >= 10_000
        ? { notation: "compact", maximumFractionDigits: 1 }
        : { maximumFractionDigits: value < 100 ? 2 : 0 },
    ).format(value);
  } catch {
    return String(Math.round(value));
  }
}

/** 没刷新成时补的一句说明（显示的是上次结果时） */
export function workbuddyStaleNote(
  t: TFunction,
  quota: WorkBuddyQuota,
): string | undefined {
  switch (quota.unavailable) {
    case "not_running":
      return t("workbuddy.quota.staleNotRunning");
    case "not_current":
      return t("workbuddy.quota.staleNotCurrent");
    case "not_signed_in":
      return t("workbuddy.quota.staleNotSignedIn");
    case "failed":
      return t("workbuddy.quota.staleFailed");
    default:
      return undefined;
  }
}

export interface WorkBuddyCreditsLine {
  line: QuotaLine;
  /** 套餐名；后端没给时为 undefined */
  planName?: string;
}

/** 积分那一行：剩余（或按设置写已用）/ 总量，条按剩余比例画，悬停写全剩余 / 总量 / 已用。 */
export function workbuddyCreditsLine(
  t: TFunction,
  quota: WorkBuddyQuota,
  locale: string,
  display: QuotaDisplay = "left",
): WorkBuddyCreditsLine | null {
  const snapshot = quota.snapshot;
  if (!snapshot) return null;
  const remaining = formatCredits(snapshot.remaining, locale);
  const used = formatCredits(snapshot.used, locale);
  const total = formatCredits(snapshot.total, locale);
  const leftPct =
    snapshot.total > 0
      ? Math.max(0, Math.min(100, (snapshot.remaining / snapshot.total) * 100))
      : 0;
  const usedUp = snapshot.remaining <= 0;
  const showUsed = display === "used";
  const value = usedUp ? t("quota.usedUp") : showUsed ? used : remaining;
  const text = usedUp
    ? t("workbuddy.quota.usedUp", { total })
    : showUsed
      ? t("workbuddy.quota.usedOf", { used, total })
      : t("workbuddy.quota.leftOf", { remaining, total });
  const planName = snapshot.planName?.trim() || undefined;
  const detail = [
    planName,
    t("workbuddy.quota.detail", { remaining, total, used }),
    workbuddyStaleNote(t, quota),
  ]
    .filter(Boolean)
    .join(" · ");
  return {
    planName,
    line: {
      key: "workbuddy_credits",
      left: leftPct,
      bar: showUsed ? 100 - leftPct : leftPct,
      tone: toneForLeft(usedUp ? 0 : leftPct),
      text,
      value,
      detail,
    },
  };
}

/**
 * 卡片右侧的额度列：积分一行；显示的是上次结果（WorkBuddy 没在运行等）时第二行写「xx 更新」。
 * 还没有任何结果时：当前账号且 WorkBuddy 没运行 → 提示打开它；查询失败 → 「额度没查到」；其余不显示。
 */
export function workbuddyCardLines(
  t: TFunction,
  quota: WorkBuddyQuota | undefined,
  locale: string,
  display: QuotaDisplay = "left",
  now = Date.now(),
): QuotaLine[] | null {
  if (!quota) return null;
  const credits = workbuddyCreditsLine(t, quota, locale, display);
  if (credits) {
    const lines = [credits.line];
    if (quota.unavailable && quota.snapshot) {
      lines.push({
        key: "workbuddy_stale",
        left: Infinity,
        tone: "muted",
        text: t("quota.updatedAt", {
          time: formatRelativeTime(quota.snapshot.queriedAt, now, t),
        }),
        detail: workbuddyStaleNote(t, quota),
      });
    }
    return lines;
  }
  if (quota.unavailable === "not_running" && quota.current) {
    return [
      {
        key: "workbuddy_not_running",
        left: Infinity,
        tone: "muted",
        text: t("workbuddy.quota.openToQuery"),
        detail: t("workbuddy.quota.onlyWhileRunning"),
      },
    ];
  }
  if (quota.unavailable === "failed") {
    return failedLines(t, quota.error || t("workbuddy.quota.unavailable"));
  }
  return null;
}
