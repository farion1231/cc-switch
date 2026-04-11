import React from "react";
import { RefreshCw, AlertCircle, Clock } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ProviderMeta } from "@/types";
import { useCopilotQuota } from "@/lib/query/copilot";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { PROVIDER_TYPES } from "@/config/constants";
import {
  TierBadge,
  utilizationColor,
} from "@/components/SubscriptionQuotaFooter";

interface CopilotQuotaFooterProps {
  meta?: ProviderMeta;
  inline?: boolean;
  /** 是否为当前激活的供应商 */
  isCurrent?: boolean;
}

/** 格式化相对时间 */
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

const CopilotQuotaFooter: React.FC<CopilotQuotaFooterProps> = ({
  meta,
  inline = false,
  isCurrent = false,
}) => {
  const { t } = useTranslation();
  const accountId = resolveManagedAccountId(
    meta,
    PROVIDER_TYPES.GITHUB_COPILOT,
  );

  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useCopilotQuota(accountId, { enabled: true, autoQuery: isCurrent });

  const [now, setNow] = React.useState(Date.now());
  React.useEffect(() => {
    if (!quota?.queriedAt) return;
    const interval = setInterval(() => setNow(Date.now()), 30000);
    return () => clearInterval(interval);
  }, [quota?.queriedAt]);

  if (!quota) return null;

  // API 调用失败
  if (!quota.success) {
    if (inline) {
      return (
        <div className="inline-flex items-center gap-2 text-xs rounded-lg border border-border-default bg-card px-3 py-2 shadow-sm">
          <div className="flex items-center gap-1.5 text-red-500 dark:text-red-400">
            <AlertCircle size={12} />
            <span>{quota.error || t("subscription.queryFailed")}</span>
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
    return null;
  }

  const tiers = quota.tiers;
  if (tiers.length === 0) return null;

  if (inline) {
    return (
      <div className="flex flex-col items-end gap-1 text-xs whitespace-nowrap flex-shrink-0">
        <div className="flex items-center gap-2 justify-end">
          {quota.plan && (
            <span className="text-[10px] text-muted-foreground/70">
              {quota.plan}
            </span>
          )}
          <span className="text-[10px] text-muted-foreground/70 flex items-center gap-1">
            <Clock size={10} />
            {quota.queriedAt
              ? formatRelativeTime(quota.queriedAt, now, t)
              : t("usage.never", { defaultValue: "Never" })}
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

        <div className="flex items-center gap-2">
          {tiers.map((tier) => (
            <TierBadge key={tier.name} tier={tier} t={t} />
          ))}
        </div>
      </div>
    );
  }

  // 展开模式
  return (
    <div className="mt-3 rounded-xl border border-border-default bg-card px-4 py-3 shadow-sm">
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs text-gray-500 dark:text-gray-400 font-medium">
          {quota.plan || t("subscription.title")}
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
        {tiers.map((tier) => {
          const label = t("subscription.copilotPremium", {
            defaultValue: "Premium",
          });
          return (
            <div key={tier.name} className="flex items-center gap-3 text-xs">
              <span
                className="text-gray-500 dark:text-gray-400 min-w-0 font-medium"
                style={{ width: "25%" }}
              >
                {label}
              </span>
              <div className="flex-1 h-2 bg-gray-100 dark:bg-gray-800 rounded-full overflow-hidden">
                <div
                  className={`h-full rounded-full transition-all ${
                    tier.utilization >= 90
                      ? "bg-red-500"
                      : tier.utilization >= 70
                        ? "bg-orange-500"
                        : "bg-green-500"
                  }`}
                  style={{
                    width: `${Math.min(tier.utilization, 100)}%`,
                  }}
                />
              </div>
              <span
                className={`font-semibold tabular-nums ${utilizationColor(tier.utilization)}`}
              >
                {Math.round(tier.utilization)}%
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
};

export default CopilotQuotaFooter;
