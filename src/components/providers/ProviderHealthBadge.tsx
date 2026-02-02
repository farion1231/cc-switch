import { useMemo } from "react";
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

  // 根据失败次数计算状态 - 使用 useMemo 避免不必要的重计算
  const statusConfig = useMemo(() => {
    if (consecutiveFailures === 0) {
      return {
        labelKey: "health.operational",
        labelFallback: "正常",
        status: ProviderHealthStatus.Healthy,
        color: "bg-green-500",
        bgColor: "bg-green-100 dark:bg-green-900/40",
        textColor: "text-green-700 dark:text-green-400",
      };
    } else if (consecutiveFailures < 5) {
      return {
        labelKey: "health.degraded",
        labelFallback: "降级",
        status: ProviderHealthStatus.Degraded,
        color: "bg-yellow-500",
        bgColor: "bg-yellow-100 dark:bg-yellow-900/40",
        textColor: "text-yellow-700 dark:text-yellow-400",
      };
    } else {
      return {
        labelKey: "health.circuitOpen",
        labelFallback: "熔断",
        status: ProviderHealthStatus.Failed,
        color: "bg-red-500",
        bgColor: "bg-red-100 dark:bg-red-900/40",
        textColor: "text-red-700 dark:text-red-400",
      };
    }
  }, [consecutiveFailures]);

  const label = t(statusConfig.labelKey, {
    defaultValue: statusConfig.labelFallback,
  });

  return (
    <div
      className={cn(
        "inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-[10px] font-medium tracking-wide",
        "transition-all duration-200 ease-out",
        statusConfig.bgColor,
        statusConfig.textColor,
        className,
      )}
      title={t("health.consecutiveFailures", {
        count: consecutiveFailures,
        defaultValue: `连续失败 ${consecutiveFailures} 次`,
      })}
    >
      <div
        className={cn(
          "w-1.5 h-1.5 rounded-full transition-colors duration-200",
          statusConfig.color,
        )}
      />
      <span>{label}</span>
    </div>
  );
}
