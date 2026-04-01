import React from "react";
import { cn } from "@/lib/utils";
import type { HealthStatus } from "@/lib/api/model-test";
import { useTranslation } from "react-i18next";

interface HealthStatusIndicatorProps {
  status: HealthStatus;
  responseTimeMs?: number;
  className?: string;
}

const statusConfig = {
  operational: {
    labelKey: "health.operational",
    labelFallback: "正常",
    color: "hsl(var(--success))",
  },
  degraded: {
    labelKey: "health.degraded",
    labelFallback: "降级",
    color: "hsl(var(--warning))",
  },
  failed: {
    labelKey: "health.failed",
    labelFallback: "失败",
    color: "hsl(var(--destructive))",
  },
};

export const HealthStatusIndicator: React.FC<HealthStatusIndicatorProps> = ({
  status,
  responseTimeMs,
  className,
}) => {
  const { t } = useTranslation();
  const config = statusConfig[status];
  const label = t(config.labelKey, { defaultValue: config.labelFallback });

  return (
    <div className={cn("flex items-center gap-2", className)}>
      <div
        className="w-2 h-2 rounded-full"
        style={{ backgroundColor: config.color }}
      />
      <span className="text-xs font-medium" style={{ color: config.color }}>
        {label}
        {responseTimeMs !== undefined && ` (${responseTimeMs}ms)`}
      </span>
    </div>
  );
};
