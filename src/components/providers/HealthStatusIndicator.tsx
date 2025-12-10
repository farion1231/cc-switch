import React from "react";
import { cn } from "@/lib/utils";
import type { HealthStatus } from "@/lib/api/model-test";

interface HealthStatusIndicatorProps {
  status: HealthStatus;
  responseTimeMs?: number;
  className?: string;
}

const statusConfig = {
  operational: {
    color: "bg-emerald-500",
    label: "正常",
    textColor: "text-emerald-600 dark:text-emerald-400",
  },
  degraded: {
    color: "bg-yellow-500",
    label: "降级",
    textColor: "text-yellow-600 dark:text-yellow-400",
  },
  failed: {
    color: "bg-red-500",
    label: "失败",
    textColor: "text-red-600 dark:text-red-400",
  },
};

export const HealthStatusIndicator: React.FC<HealthStatusIndicatorProps> = ({
  status,
  responseTimeMs,
  className,
}) => {
  const config = statusConfig[status];

  return (
    <div className={cn("flex items-center gap-2", className)}>
      <div className={cn("w-2 h-2 rounded-full", config.color)} />
      <span className={cn("text-xs font-medium", config.textColor)}>
        {config.label}
        {responseTimeMs !== undefined && ` (${responseTimeMs}ms)`}
      </span>
    </div>
  );
};
