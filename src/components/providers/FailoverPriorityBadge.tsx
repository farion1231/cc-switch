import { memo } from "react";
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
export const FailoverPriorityBadge = memo(function FailoverPriorityBadge({
  priority,
  className,
}: FailoverPriorityBadgeProps) {
  const { t } = useTranslation();

  return (
    <div
      className={cn(
        "inline-flex items-center px-2 py-0.5 rounded-full text-[10px] font-semibold tracking-wide",
        "bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-400",
        "transition-opacity duration-200 ease-out",
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
});
