import React from "react";
import { RefreshCw, AlertCircle, Clock } from "lucide-react";
import { useTranslation } from "react-i18next";
import { type AppId } from "@/lib/api";
import { useUsageQuery } from "@/lib/query/queries";
import { UsageData, Provider } from "@/types";
import { TierBadge } from "@/components/SubscriptionQuotaFooter";
import type { QuotaTier } from "@/types/subscription";

interface UsageFooterProps {
  provider: Provider;
  providerId: string;
  appId: AppId;
  usageEnabled: boolean; // 是否启用了用量查询
  isCurrent: boolean; // 是否为当前激活的供应商
  isInConfig?: boolean; // OpenCode: 是否已添加到配置
  inline?: boolean; // 是否内联显示（在按钮左侧）
}

/** UsageData → QuotaTier 转换（Token Plan 使用） */
function toQuotaTier(data: UsageData): QuotaTier {
  return {
    name: data.planName || "",
    utilization: data.used || 0,
    resetsAt: data.extra || null,
  };
}

const UsageFooter: React.FC<UsageFooterProps> = ({
  provider,
  providerId,
  appId,
  usageEnabled,
  isCurrent,
  isInConfig = false,
  inline = false,
}) => {
  const { t } = useTranslation();
  const isTokenPlan =
    provider.meta?.usage_script?.templateType === "token_plan";

  // 统一的用量查询（自动查询仅对当前激活的供应商启用）
  // OpenCode（累加模式）：使用 isInConfig 代替 isCurrent
  const shouldAutoQuery = appId === "opencode" ? isInConfig : isCurrent;
  const autoQueryInterval = shouldAutoQuery
    ? provider.meta?.usage_script?.autoQueryInterval || 0
    : 0;

  const {
    data: usage,
    isFetching: loading,
    lastQueriedAt,
    refetch,
  } = useUsageQuery(providerId, appId, {
    enabled: usageEnabled,
    autoQueryInterval,
  });

  // 🆕 定期更新当前时间，用于刷新相对时间显示
  const [now, setNow] = React.useState(Date.now());

  React.useEffect(() => {
    if (!lastQueriedAt) return;

    // 每30秒更新一次当前时间，触发相对时间显示的刷新
    const interval = setInterval(() => {
      setNow(Date.now());
    }, 30000); // 30秒

    return () => clearInterval(interval);
  }, [lastQueriedAt]);

  // 只在启用用量查询且有数据时显示
  if (!usageEnabled || !usage) return null;

  // 错误状态
  if (!usage.success) {
    if (inline) {
      return (
        <div className="inline-flex items-center gap-2 text-xs rounded-lg border border-border-default bg-card px-3 py-2 shadow-sm">
          <div className="flex items-center gap-1.5 text-destructive">
            <AlertCircle size={12} />
            <span>{t("usage.queryFailed")}</span>
          </div>
          <button
            onClick={() => refetch()}
            disabled={loading}
            className="p-1 rounded hover:bg-muted transition-colors disabled:opacity-50 flex-shrink-0"
            title={t("usage.refreshUsage")}
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          </button>
        </div>
      );
    }

    return (
      <div className="mt-3 rounded-xl border border-border-default bg-card px-4 py-3 shadow-sm">
        <div className="flex items-center justify-between gap-2 text-xs">
          <div className="flex items-center gap-2 text-destructive">
            <AlertCircle size={14} />
            <span>{usage.error || t("usage.queryFailed")}</span>
          </div>

          {/* 刷新按钮 */}
          <button
            onClick={() => refetch()}
            disabled={loading}
            className="p-1 rounded hover:bg-muted transition-colors disabled:opacity-50 flex-shrink-0"
            title={t("usage.refreshUsage")}
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          </button>
        </div>
      </div>
    );
  }

  const usageDataList = usage.data || [];

  // 无数据时不显示
  if (usageDataList.length === 0) return null;

  // ── Token Plan：订阅风格内联渲染（百分比徽章 + 倒计时） ──
  if (isTokenPlan && inline) {
    return (
      <div className="flex flex-col items-end gap-1 text-xs whitespace-nowrap flex-shrink-0">
        {/* 第一行：查询时间 + 刷新 */}
        <div className="flex items-center gap-2 justify-end">
          <span className="text-[10px] text-muted-foreground/70 flex items-center gap-1">
            <Clock size={10} />
            {lastQueriedAt
              ? formatRelativeTime(lastQueriedAt, now, t)
              : t("usage.never", { defaultValue: "从未更新" })}
          </span>
          <button
            onClick={(e) => {
              e.stopPropagation();
              refetch();
            }}
            disabled={loading}
            className="p-1 rounded hover:bg-muted transition-colors disabled:opacity-50 flex-shrink-0 text-muted-foreground"
            title={t("usage.refreshUsage")}
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          </button>
        </div>
        {/* 第二行：tier 徽章（复用官方订阅的 TierBadge） */}
        <div className="flex items-center gap-2">
          {usageDataList.map((data, index) => (
            <TierBadge key={index} tier={toQuotaTier(data)} t={t} />
          ))}
        </div>
      </div>
    );
  }

  // ── 通用用量：内联模式（原有逻辑） ──
  if (inline) {
    const firstUsage = usageDataList[0];
    const isExpired = firstUsage.isValid === false;

    return (
      <div className="flex flex-col items-end gap-1 text-xs whitespace-nowrap flex-shrink-0">
        {/* 第一行：更新时间和刷新按钮 */}
        <div className="flex items-center gap-2 justify-end">
          {/* 上次查询时间 */}
          <span className="text-[10px] text-muted-foreground/70 flex items-center gap-1">
            <Clock size={10} />
            {lastQueriedAt
              ? formatRelativeTime(lastQueriedAt, now, t)
              : t("usage.never", { defaultValue: "从未更新" })}
          </span>

          {/* 刷新按钮 */}
          <button
            onClick={(e) => {
              e.stopPropagation();
              refetch();
            }}
            disabled={loading}
            className="p-1 rounded hover:bg-muted transition-colors disabled:opacity-50 flex-shrink-0 text-muted-foreground"
            title={t("usage.refreshUsage")}
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          </button>
        </div>

        {/* 第二行：用量和剩余 */}
        <div className="flex items-center gap-2">
          {/* 已用 */}
          {firstUsage.used !== undefined && (
            <div className="flex items-center gap-0.5">
              <span className="text-muted-foreground">{t("usage.used")}</span>
              <span className="tabular-nums font-medium text-foreground/80">
                {firstUsage.used.toFixed(2)}
              </span>
            </div>
          )}

          {/* 剩余 */}
          {firstUsage.remaining !== undefined && (
            <div className="flex items-center gap-0.5">
              <span className="text-muted-foreground">
                {t("usage.remaining")}
              </span>
              <span
                className={`font-semibold tabular-nums ${
                  isExpired
                    ? "text-destructive"
                    : firstUsage.remaining <
                        (firstUsage.total || firstUsage.remaining) * 0.1
                      ? "text-amber-500 dark:text-amber-400"
                      : "text-emerald-600 dark:text-emerald-400"
                }`}
              >
                {firstUsage.remaining.toFixed(2)}
              </span>
            </div>
          )}

          {/* 单位 */}
          {firstUsage.unit && (
            <span className="text-muted-foreground">{firstUsage.unit}</span>
          )}

          {/* 扩展字段 extra */}
          {firstUsage.extra && (
            <span
              className="max-w-[150px] truncate text-muted-foreground"
              title={firstUsage.extra}
            >
              {firstUsage.extra}
            </span>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="mt-3 rounded-xl border border-border-default bg-card px-4 py-3 shadow-sm">
      {/* 标题行：包含刷新按钮和自动查询时间 */}
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs font-medium text-muted-foreground">
          {t("usage.planUsage")}
        </span>
        <div className="flex items-center gap-2">
          {/* 自动查询时间提示 */}
          {lastQueriedAt && (
            <span className="text-[10px] text-muted-foreground/70 flex items-center gap-1">
              <Clock size={10} />
              {formatRelativeTime(lastQueriedAt, now, t)}
            </span>
          )}
          <button
            onClick={() => refetch()}
            disabled={loading}
            className="p-1 rounded hover:bg-muted transition-colors disabled:opacity-50"
            title={t("usage.refreshUsage")}
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          </button>
        </div>
      </div>

      {/* 套餐列表 */}
      <div className="flex flex-col gap-3">
        {usageDataList.map((usageData, index) => (
          <UsagePlanItem key={index} data={usageData} />
        ))}
      </div>
    </div>
  );
};

// ── 通用用量组件 ────────────────────────────────────────────

// 单个套餐数据展示组件
const UsagePlanItem: React.FC<{ data: UsageData }> = ({ data }) => {
  const { t } = useTranslation();
  const {
    planName,
    extra,
    isValid,
    invalidMessage,
    total,
    used,
    remaining,
    unit,
  } = data;

  // 判断套餐是否失效（isValid 为 false 或未定义时视为有效）
  const isExpired = isValid === false;

  return (
    <div className="flex items-center gap-3">
      {/* 标题部分：25% */}
      <div
        className="min-w-0 text-xs text-muted-foreground"
        style={{ width: "25%" }}
      >
        {planName ? (
          <span
            className={`block truncate font-medium ${isExpired ? "text-destructive" : "text-foreground"}`}
            title={planName}
          >
            💰 {planName}
          </span>
        ) : (
          <span className="opacity-50">—</span>
        )}
      </div>

      {/* 扩展字段：30% */}
      <div
        className="min-w-0 flex items-center gap-2 text-xs text-muted-foreground"
        style={{ width: "30%" }}
      >
        {extra && (
          <span
            className={`truncate ${isExpired ? "text-destructive" : ""}`}
            title={extra}
          >
            {extra}
          </span>
        )}
        {isExpired && (
          <span className="flex-shrink-0 rounded border border-destructive/20 bg-destructive/10 px-1.5 py-0.5 text-[10px] font-medium text-destructive">
            {invalidMessage || t("usage.invalid")}
          </span>
        )}
      </div>

      {/* 用量信息：45% */}
      <div
        className="flex items-center justify-end gap-2 text-xs flex-shrink-0"
        style={{ width: "45%" }}
      >
        {/* 总额度 */}
        {total !== undefined && (
          <>
            <span className="text-muted-foreground">{t("usage.total")}</span>
            <span className="tabular-nums text-foreground/80">
              {total === -1 ? "∞" : total.toFixed(2)}
            </span>
            <span className="text-muted-foreground/50">|</span>
          </>
        )}

        {/* 已用额度 */}
        {used !== undefined && (
          <>
            <span className="text-muted-foreground">{t("usage.used")}</span>
            <span className="tabular-nums text-foreground/80">
              {used.toFixed(2)}
            </span>
            <span className="text-muted-foreground/50">|</span>
          </>
        )}

        {/* 剩余额度 - 突出显示 */}
        {remaining !== undefined && (
          <>
            <span className="text-muted-foreground">
              {t("usage.remaining")}
            </span>
            <span
              className={`font-semibold tabular-nums ${
                isExpired
                  ? "text-destructive"
                  : remaining < (total || remaining) * 0.1
                    ? "text-amber-500 dark:text-amber-400"
                    : "text-emerald-600 dark:text-emerald-400"
              }`}
            >
              {remaining.toFixed(2)}
            </span>
          </>
        )}

        {unit && <span className="text-muted-foreground">{unit}</span>}
      </div>
    </div>
  );
};

// 格式化相对时间
function formatRelativeTime(
  timestamp: number,
  now: number,
  t: (key: string, options?: { count?: number }) => string,
): string {
  const diff = Math.floor((now - timestamp) / 1000); // 秒

  if (diff < 60) {
    return t("usage.justNow");
  } else if (diff < 3600) {
    const minutes = Math.floor(diff / 60);
    return t("usage.minutesAgo", { count: minutes });
  } else if (diff < 86400) {
    const hours = Math.floor(diff / 3600);
    return t("usage.hoursAgo", { count: hours });
  } else {
    const days = Math.floor(diff / 86400);
    return t("usage.daysAgo", { count: days });
  }
}

export default UsageFooter;
