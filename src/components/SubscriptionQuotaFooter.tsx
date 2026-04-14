import React from "react";
import { RefreshCw, AlertCircle, Clock } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";
import { useSubscriptionQuota } from "@/lib/query/subscription";
import type { QuotaTier, SubscriptionQuota } from "@/types/subscription";

interface SubscriptionQuotaFooterProps {
  appId: AppId;
  inline?: boolean;
  isCurrent?: boolean;
}

interface SubscriptionQuotaViewProps {
  quota: SubscriptionQuota | undefined;
  loading: boolean;
  refetch: () => void;
  /** 用于 `subscription.expiredHint` 的 {tool} 插值；解耦了 hook 的 appId */
  appIdForExpiredHint: string;
  inline?: boolean;
}

/** 已知 tier 名称的显示映射（官方订阅 + Token Plan 共用） */
export const TIER_I18N_KEYS: Record<string, string> = {
  five_hour: "subscription.fiveHour",
  seven_day: "subscription.sevenDay",
  seven_day_opus: "subscription.sevenDayOpus",
  seven_day_sonnet: "subscription.sevenDaySonnet",
  // Gemini 模型分类
  gemini_pro: "subscription.geminiPro",
  gemini_flash: "subscription.geminiFlash",
  gemini_flash_lite: "subscription.geminiFlashLite",
  // Token Plan（five_hour 已在上方官方映射中）
  weekly_limit: "subscription.weeklyLimit",
  // GitHub Copilot
  premium: "subscription.copilotPremium",
};

/** 根据使用百分比返回颜色 class */
export function utilizationColor(utilization: number): string {
  if (utilization >= 90) return "text-red-500 dark:text-red-400";
  if (utilization >= 70) return "text-orange-500 dark:text-orange-400";
  return "text-green-600 dark:text-green-400";
}

/** 计算倒计时的纯时间字符串，如 "2h30m"、"3d12h" */
export function countdownStr(resetsAt: string | null): string | null {
  if (!resetsAt) return null;
  const diffMs = new Date(resetsAt).getTime() - Date.now();
  if (diffMs <= 0) return null;

  const hours = Math.floor(diffMs / (1000 * 60 * 60));
  const minutes = Math.floor((diffMs % (1000 * 60 * 60)) / (1000 * 60));

  if (hours > 24) {
    const days = Math.floor(hours / 24);
    return `${days}d${hours % 24}h`;
  }
  if (hours > 0) return `${hours}h${minutes}m`;
  return `${minutes}m`;
}

/** 格式化重置时间为倒计时文本（带 i18n 模板） */
function formatResetTime(
  resetsAt: string | null,
  t: (key: string, options?: Record<string, string>) => string,
): string | null {
  const time = countdownStr(resetsAt);
  if (!time) return null;
  return t("subscription.resetsIn", { time });
}

/** 不需要在 inline 模式显示的 tier */
const HIDDEN_INLINE_TIERS = new Set(["seven_day_sonnet"]);

/** 格式化相对时间（与 UsageFooter 一致） */
function formatRelativeTime(
  timestamp: number,
  now: number,
  t: (key: string, options?: { count?: number }) => string,
): string {
  const diff = Math.floor((now - timestamp) / 1000);
  if (diff < 60) return t("usage.justNow");
  if (diff < 3600)
    return t("usage.minutesAgo", { count: Math.floor(diff / 60) });
  if (diff < 86400)
    return t("usage.hoursAgo", { count: Math.floor(diff / 3600) });
  return t("usage.daysAgo", { count: Math.floor(diff / 86400) });
}

/**
 * 纯展示组件：渲染 SubscriptionQuota 的 5 种状态（not_found / parse_error /
 * expired / API 失败 / 成功），支持 inline / expanded 两种布局。
 *
 * 数据源由调用方 hook 注入，方便不同的额度后端复用同一套渲染逻辑：
 * - `SubscriptionQuotaFooter`（CLI 凭据路径，by appId）
 * - `CodexOauthQuotaFooter`（cc-switch 自管 OAuth 路径，by ChatGPT account）
 */
export const SubscriptionQuotaView: React.FC<SubscriptionQuotaViewProps> = ({
  quota,
  loading,
  refetch,
  appIdForExpiredHint,
  inline = false,
}) => {
  const { t } = useTranslation();

  // 定期更新相对时间显示
  const [now, setNow] = React.useState(Date.now());
  React.useEffect(() => {
    if (!quota?.queriedAt) return;
    const interval = setInterval(() => setNow(Date.now()), 30000);
    return () => clearInterval(interval);
  }, [quota?.queriedAt]);

  // 无凭据 → 不显示
  if (!quota || quota.credentialStatus === "not_found") return null;

  // 凭据解析错误 → 不显示（静默）
  if (quota.credentialStatus === "parse_error") return null;

  // 凭据过期
  if (quota.credentialStatus === "expired" && !quota.success) {
    if (inline) {
      return (
        <div className="inline-flex items-center gap-2 text-xs rounded-lg border border-amber-200 dark:border-amber-800 bg-amber-50 dark:bg-amber-900/20 px-3 py-2 shadow-sm">
          <div className="flex items-center gap-1.5 text-amber-600 dark:text-amber-400">
            <AlertCircle size={12} />
            <span>{t("subscription.expired")}</span>
          </div>
          <button
            onClick={() => refetch()}
            disabled={loading}
            className="p-1 rounded hover:bg-muted transition-colors disabled:opacity-50 flex-shrink-0"
            title={t("subscription.refresh")}
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          </button>
        </div>
      );
    }
    return (
      <div className="mt-3 rounded-xl border border-amber-200 dark:border-amber-800 bg-amber-50 dark:bg-amber-900/20 px-4 py-3 shadow-sm">
        <div className="flex items-center justify-between gap-2 text-xs">
          <div className="flex items-center gap-2 text-amber-600 dark:text-amber-400">
            <AlertCircle size={14} />
            <div>
              <span className="font-medium">{t("subscription.expired")}</span>
              <span className="ml-2 text-amber-500/70 dark:text-amber-400/70">
                {t("subscription.expiredHint", { tool: appIdForExpiredHint })}
              </span>
            </div>
          </div>
          <button
            onClick={() => refetch()}
            disabled={loading}
            className="p-1 rounded hover:bg-amber-100 dark:hover:bg-amber-800/30 transition-colors disabled:opacity-50 flex-shrink-0"
            title={t("subscription.refresh")}
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          </button>
        </div>
      </div>
    );
  }

  // API 调用失败
  if (!quota.success) {
    if (inline) {
      return (
        <div className="inline-flex items-center gap-2 text-xs rounded-lg border border-border-default bg-card px-3 py-2 shadow-sm">
          <div className="flex items-center gap-1.5 text-red-500 dark:text-red-400">
            <AlertCircle size={12} />
            <span>{t("subscription.queryFailed")}</span>
          </div>
          <button
            onClick={() => refetch()}
            disabled={loading}
            className="p-1 rounded hover:bg-muted transition-colors disabled:opacity-50 flex-shrink-0"
            title={t("subscription.refresh")}
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          </button>
        </div>
      );
    }
    return (
      <div className="mt-3 rounded-xl border border-border-default bg-card px-4 py-3 shadow-sm">
        <div className="flex items-center justify-between gap-2 text-xs">
          <div className="flex items-center gap-2 text-red-500 dark:text-red-400">
            <AlertCircle size={14} />
            <span>{quota.error || t("subscription.queryFailed")}</span>
          </div>
          <button
            onClick={() => refetch()}
            disabled={loading}
            className="p-1 rounded hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors disabled:opacity-50 flex-shrink-0"
            title={t("subscription.refresh")}
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          </button>
        </div>
      </div>
    );
  }

  // 成功获取数据
  const tiers = quota.tiers || [];
  if (tiers.length === 0) return null;

  // ── inline 模式：紧凑两行显示 ──
  if (inline) {
    return (
      <div className="flex flex-col items-end gap-1 text-xs whitespace-nowrap flex-shrink-0">
        {/* 第一行：查询时间 + 刷新 */}
        <div className="flex items-center gap-2 justify-end">
          <span className="text-[10px] text-muted-foreground/70 flex items-center gap-1">
            <Clock size={10} />
            {quota.queriedAt
              ? formatRelativeTime(quota.queriedAt, now, t)
              : t("usage.never", { defaultValue: "从未更新" })}
          </span>
          <button
            onClick={(e) => {
              e.stopPropagation();
              refetch();
            }}
            disabled={loading}
            className="p-1 rounded hover:bg-muted transition-colors disabled:opacity-50 flex-shrink-0 text-muted-foreground"
            title={t("subscription.refresh")}
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          </button>
        </div>

        {/* 第二行：各 tier 使用百分比 */}
        <div className="flex items-center gap-2">
          {tiers
            .filter((tier) => !HIDDEN_INLINE_TIERS.has(tier.name))
            .map((tier) => (
              <TierBadge key={tier.name} tier={tier} t={t} />
            ))}
        </div>
      </div>
    );
  }

  // ── 展开模式：详细信息 ──
  return (
    <div className="mt-3 rounded-xl border border-border-default bg-card px-4 py-3 shadow-sm">
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs text-gray-500 dark:text-gray-400 font-medium">
          {t("subscription.title", { defaultValue: "Subscription Quota" })}
        </span>
        <div className="flex items-center gap-2">
          {quota.queriedAt && (
            <span className="text-[10px] text-muted-foreground/70 flex items-center gap-1">
              <Clock size={10} />
              {formatRelativeTime(quota.queriedAt, now, t)}
            </span>
          )}
          <button
            onClick={() => refetch()}
            disabled={loading}
            className="p-1 rounded hover:bg-muted transition-colors disabled:opacity-50"
            title={t("subscription.refresh")}
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          </button>
        </div>
      </div>

      <div className="flex flex-col gap-2">
        {tiers.map((tier) => (
          <TierBar key={tier.name} tier={tier} t={t} />
        ))}
      </div>

      {/* 超额使用 */}
      {quota.extraUsage?.isEnabled && (
        <div className="mt-2 pt-2 border-t border-border-default text-xs text-gray-500 dark:text-gray-400">
          <span className="font-medium">{t("subscription.extraUsage")}: </span>
          <span className="tabular-nums">
            {quota.extraUsage.currency === "USD" ? "$" : ""}
            {(quota.extraUsage.usedCredits ?? 0).toFixed(2)}
            {quota.extraUsage.monthlyLimit != null && (
              <>
                {" "}
                / {quota.extraUsage.currency === "USD" ? "$" : ""}
                {quota.extraUsage.monthlyLimit.toFixed(2)}
              </>
            )}
          </span>
        </div>
      )}
    </div>
  );
};

/** inline 模式下的单个 tier 显示 */
export const TierBadge: React.FC<{
  tier: QuotaTier;
  t: (key: string, options?: Record<string, unknown>) => string;
}> = ({ tier, t }) => {
  const label = TIER_I18N_KEYS[tier.name]
    ? t(TIER_I18N_KEYS[tier.name])
    : tier.name;
  const countdown = countdownStr(tier.resetsAt);

  return (
    <div className="flex items-center gap-0.5">
      <span className="text-gray-500 dark:text-gray-400">{label}:</span>
      <span
        className={`font-semibold tabular-nums ${utilizationColor(tier.utilization)}`}
      >
        {t("subscription.utilization", { value: Math.round(tier.utilization) })}
      </span>
      {countdown && (
        <span className="text-muted-foreground/60 ml-0.5 flex items-center gap-px">
          <Clock size={10} />
          {countdown}
        </span>
      )}
    </div>
  );
};

/** 展开模式下的单个 tier 进度条 */
const TierBar: React.FC<{
  tier: QuotaTier;
  t: (key: string, options?: Record<string, unknown>) => string;
}> = ({ tier, t }) => {
  const label = TIER_I18N_KEYS[tier.name]
    ? t(TIER_I18N_KEYS[tier.name])
    : tier.name;
  const resetText = formatResetTime(tier.resetsAt, t);

  return (
    <div className="flex items-center gap-3 text-xs">
      <span
        className="text-gray-500 dark:text-gray-400 min-w-0 font-medium"
        style={{ width: "25%" }}
      >
        {label}
      </span>

      {/* 进度条 */}
      <div className="flex-1 h-2 bg-gray-100 dark:bg-gray-800 rounded-full overflow-hidden">
        <div
          className={`h-full rounded-full transition-all ${
            tier.utilization >= 90
              ? "bg-red-500"
              : tier.utilization >= 70
                ? "bg-orange-500"
                : "bg-green-500"
          }`}
          style={{ width: `${Math.min(tier.utilization, 100)}%` }}
        />
      </div>

      <div
        className="flex items-center gap-2 flex-shrink-0"
        style={{ width: "30%" }}
      >
        <span
          className={`font-semibold tabular-nums ${utilizationColor(tier.utilization)}`}
        >
          {Math.round(tier.utilization)}%
        </span>
        {resetText && (
          <span
            className="text-[10px] text-muted-foreground/70 truncate"
            title={resetText}
          >
            {resetText}
          </span>
        )}
      </div>
    </div>
  );
};

/**
 * CLI 凭据路径下的薄 wrapper：通过 useSubscriptionQuota(appId) 自取数据
 * 后转发到 SubscriptionQuotaView。对外 props/行为与重构前完全一致。
 */
const SubscriptionQuotaFooter: React.FC<SubscriptionQuotaFooterProps> = ({
  appId,
  inline = false,
  isCurrent = false,
}) => {
  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useSubscriptionQuota(appId, isCurrent, isCurrent);

  if (!isCurrent) return null;

  return (
    <SubscriptionQuotaView
      quota={quota}
      loading={loading}
      refetch={refetch}
      appIdForExpiredHint={appId}
      inline={inline}
    />
  );
};

export default SubscriptionQuotaFooter;
