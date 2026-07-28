import { AlertTriangle, Gauge } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import type { CodexQuotaForecast } from "@/types/subscription";
import { cn } from "@/lib/utils";

export function CodexQuotaForecastBadge({
  forecast,
}: {
  forecast?: CodexQuotaForecast;
}) {
  const { t } = useTranslation();
  if (!forecast) return null;

  const learning = forecast.estimatedFiveHourWindowsRemaining == null;
  const atRisk = forecast.willExhaustBeforeReset === true;
  const estimatedWindows = forecast.estimatedFiveHourWindowsRemaining ?? 0;
  const label = learning
    ? t("codexQuota.forecastLearning", {
        defaultValue: "Forecast learning {{current}}/{{required}}",
        current: forecast.sampleCount,
        required: forecast.minimumSampleCount,
      })
    : atRisk
      ? t("codexQuota.forecastAtRisk", {
          defaultValue: "May run out before weekly reset",
        })
      : t("codexQuota.forecastWindows", {
          defaultValue: "About {{windows}} full 5h windows left",
          windows: estimatedWindows.toFixed(1),
        });

  const Icon = atRisk ? AlertTriangle : Gauge;
  return (
    <Badge
      variant="outline"
      className={cn(
        "h-5 gap-1 px-1.5 text-[10px] font-normal",
        atRisk && "border-amber-500/40 text-amber-600 dark:text-amber-400",
      )}
    >
      <Icon className="h-3 w-3" />
      {label}
    </Badge>
  );
}
