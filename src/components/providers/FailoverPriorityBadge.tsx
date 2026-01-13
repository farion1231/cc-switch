import { cn } from "@/lib/utils";
import { useTranslation } from "react-i18next";

interface FailoverPriorityBadgeProps {
  priority: number; // 1, 2, 3, ...
  className?: string;
}

/**
 * 故障转移优先级徽章
 * 显示供应商在故障转移队列中的优先级顺序
 */
export function FailoverPriorityBadge({
  priority,
  className,
}: FailoverPriorityBadgeProps) {
  const { t } = useTranslation();

  return (
    <div
      className={cn(
        "inline-flex items-center px-1.5 py-0.5 rounded text-xs font-semibold",
        "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
        className,
      )}
      title={t("failover.priority.tooltip", {
        priority,
        defaultValue: `故障转移优先级 ${priority}`,
      })}
    >
      P{priority}
    </div>
  );
}
