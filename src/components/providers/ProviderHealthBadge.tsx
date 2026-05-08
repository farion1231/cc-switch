import { cn } from "@/lib/utils";
import { ProviderHealthStatus } from "@/types/proxy";
import { useTranslation } from "react-i18next";

interface ProviderHealthBadgeProps {
  consecutiveFailures: number;
  className?: string;
}

/**
 * 供应商健康状态徽章
 * 根据连续失败次数显示不同颜色的状态指示器
 */
export function ProviderHealthBadge({
  consecutiveFailures,
  className,
}: ProviderHealthBadgeProps) {
  const { t } = useTranslation();

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
    } else if (consecutiveFailures < 5) {
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

  return (
    <div
      className={cn(
        "inline-flex items-center gap-1.5 px-2 py-1 rounded-full text-xs font-medium",
        statusConfig.bgColor,
        statusConfig.textColor,
        className,
      )}
      title={t("health.consecutiveFailures", {
        count: consecutiveFailures,
        defaultValue: `连续失败 ${consecutiveFailures} 次`,
      })}
    >
      <div className={cn("w-2 h-2 rounded-full", statusConfig.color)} />
      <span>{label}</span>
    </div>
  );
}
