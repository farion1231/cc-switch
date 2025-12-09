import { cn } from "@/lib/utils";
import { ProviderHealthStatus } from "@/types/proxy";

interface ProviderHealthBadgeProps {
  consecutiveFailures: number;
  isProxyTarget?: boolean;
  className?: string;
}

/**
 * 供应商健康状态徽章
 * 根据连续失败次数显示不同颜色的状态指示器
 */
export function ProviderHealthBadge({
  consecutiveFailures,
  isProxyTarget,
  className,
}: ProviderHealthBadgeProps) {
  // 如果代理目标已关闭但有失败记录，仍然显示（自动熔断场景）
  // 如果代理目标启用，始终显示
  // 如果代理目标关闭且无失败记录，隐藏
  if (!isProxyTarget && consecutiveFailures === 0) return null;

  // 根据失败次数计算状态
  const getStatus = () => {
    if (consecutiveFailures === 0) {
      return {
        label: "正常",
        status: ProviderHealthStatus.Healthy,
        color: "bg-green-500",
        // 使用更深/柔和的背景色，去除可能的白色内容感
        bgColor: "bg-green-500/10",
        textColor: "text-green-600 dark:text-green-400",
      };
    } else if (consecutiveFailures < 5) {
      return {
        label: "降级",
        status: ProviderHealthStatus.Degraded,
        color: "bg-yellow-500",
        bgColor: "bg-yellow-500/10",
        textColor: "text-yellow-600 dark:text-yellow-400",
      };
    } else {
      return {
        label: "熔断",
        status: ProviderHealthStatus.Failed,
        color: "bg-red-500",
        bgColor: "bg-red-500/10",
        textColor: "text-red-600 dark:text-red-400",
      };
    }
  };

  const statusConfig = getStatus();

  return (
    <div
      className={cn(
        "inline-flex items-center gap-1.5 px-2 py-1 rounded-full text-xs font-medium",
        statusConfig.bgColor,
        statusConfig.textColor,
        className,
      )}
      title={`连续失败 ${consecutiveFailures} 次`}
    >
      <div className={cn("w-2 h-2 rounded-full", statusConfig.color)} />
      <span>{statusConfig.label}</span>
    </div>
  );
}
