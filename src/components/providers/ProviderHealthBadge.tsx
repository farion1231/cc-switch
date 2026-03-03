import { cn } from "@/lib/utils";
import { ProviderHealthStatus } from "@/types/proxy";
import { useTranslation } from "react-i18next";

interface ProviderHealthBadgeProps {
  consecutiveFailures: number;
  failureThreshold?: number;
  lastError?: string | null;
  className?: string;
}

/**
 * 供应商健康状态徽章
 * 根据连续失败次数显示不同颜色的状态指示器
 */
export function ProviderHealthBadge({
  consecutiveFailures,
  failureThreshold = 4,
  lastError,
  className,
}: ProviderHealthBadgeProps) {
  const { t } = useTranslation();
  const normalizedThreshold = Math.max(1, failureThreshold);

  const getErrorHint = () => {
    if (!lastError) return null;
    const error = lastError.toLowerCase();
    if (error.includes("401") || error.includes("invalid_api_key")) {
      return t("health.authErrorHint", {
        defaultValue: "可能是认证问题（401）",
      });
    }
    if (
      error.includes("429") ||
      error.includes("usage_limit_reached") ||
      error.includes("rate limit")
    ) {
      return t("health.rateLimitHint", {
        defaultValue: "可能是额度/速率限制（429）",
      });
    }
    return t("health.lastErrorHint", {
      defaultValue: `最近错误: ${lastError}`,
      error: lastError,
    });
  };

  // 根据失败次数计算状态
  const getStatus = () => {
    if (consecutiveFailures === 0) {
      return {
        labelKey: "health.operational",
        labelFallback: "正常",
        status: ProviderHealthStatus.Healthy,
        color: "bg-green-500",
        // 使用更深/柔和的背景色，去除可能的白色内容感
        bgColor: "bg-green-500/10",
        textColor: "text-green-600 dark:text-green-400",
      };
    } else if (consecutiveFailures < normalizedThreshold) {
      return {
        labelKey: "health.degraded",
        labelFallback: "降级",
        status: ProviderHealthStatus.Degraded,
        color: "bg-yellow-500",
        bgColor: "bg-yellow-500/10",
        textColor: "text-yellow-600 dark:text-yellow-400",
      };
    } else {
      return {
        labelKey: "health.circuitOpen",
        labelFallback: "熔断",
        status: ProviderHealthStatus.Failed,
        color: "bg-red-500",
        bgColor: "bg-red-500/10",
        textColor: "text-red-600 dark:text-red-400",
      };
    }
  };

  const statusConfig = getStatus();
  const label = t(statusConfig.labelKey, {
    defaultValue: statusConfig.labelFallback,
  });
  const baseTitle = t("health.consecutiveFailures", {
    count: consecutiveFailures,
    defaultValue: `连续失败 ${consecutiveFailures} 次`,
  });
  const errorHint = getErrorHint();
  const title = errorHint ? `${baseTitle}\n${errorHint}` : baseTitle;

  return (
    <div
      className={cn(
        "inline-flex items-center gap-1.5 px-2 py-1 rounded-full text-xs font-medium",
        statusConfig.bgColor,
        statusConfig.textColor,
        className,
      )}
      title={title}
    >
      <div className={cn("w-2 h-2 rounded-full", statusConfig.color)} />
      <span>{label}</span>
    </div>
  );
}
