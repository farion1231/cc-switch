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
        statusColor: "hsl(var(--success))",
        backgroundColor: "hsl(var(--success) / 0.12)",
      };
    } else if (consecutiveFailures < 5) {
      return {
        labelKey: "health.degraded",
        labelFallback: "降级",
        status: ProviderHealthStatus.Degraded,
        statusColor: "hsl(var(--warning))",
        backgroundColor: "hsl(var(--warning) / 0.12)",
      };
    } else {
      return {
        labelKey: "health.circuitOpen",
        labelFallback: "熔断",
        status: ProviderHealthStatus.Failed,
        statusColor: "hsl(var(--destructive))",
        backgroundColor: "hsl(var(--destructive) / 0.12)",
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
        className,
      )}
      style={{
        backgroundColor: statusConfig.backgroundColor,
        color: statusConfig.statusColor,
      }}
      title={t("health.consecutiveFailures", {
        count: consecutiveFailures,
        defaultValue: `连续失败 ${consecutiveFailures} 次`,
      })}
    >
      <div
        className="h-2 w-2 rounded-full"
        style={{ backgroundColor: statusConfig.statusColor }}
      />
      <span>{label}</span>
    </div>
  );
}
