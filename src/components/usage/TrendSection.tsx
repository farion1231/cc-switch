/**
 * Trend section: the usage chart and the quota chart behind one switch
 * (additive wrapper — the dashboard renders this instead of `UsageTrendChart`
 * directly, and both charts keep their own props).
 *
 * The two answer different questions over the same date range — "how much did I
 * spend" vs "how full were the subscription windows" — so they share the slot
 * rather than stacking, with the active side highlighted.
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import type { UsageRangeSelection } from "@/types/usage";
import { UsageTrendChart } from "./UsageTrendChart";
import { QuotaTrendChart } from "./QuotaTrendChart";

const MODE_STORAGE_KEY = "cc-switch:trend-mode";

type TrendMode = "usage" | "quota";

function readSavedMode(): TrendMode {
  try {
    return localStorage.getItem(MODE_STORAGE_KEY) === "quota"
      ? "quota"
      : "usage";
  } catch {
    return "usage";
  }
}

interface TrendSectionProps {
  range: UsageRangeSelection;
  rangeLabel: string;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

export function TrendSection({
  range,
  rangeLabel,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: TrendSectionProps) {
  const { t } = useTranslation();
  const [mode, setMode] = useState<TrendMode>(readSavedMode);

  const changeMode = (next: TrendMode) => {
    setMode(next);
    try {
      localStorage.setItem(MODE_STORAGE_KEY, next);
    } catch {
      // Storage denied — the switch still works for this session.
    }
  };

  const toggle = (
    <div className="flex items-center gap-2 text-xs">
      <button
        type="button"
        onClick={() => changeMode("usage")}
        className={cn(
          "transition-colors",
          mode === "usage"
            ? "font-medium text-foreground"
            : "text-muted-foreground hover:text-foreground",
        )}
      >
        {t("usage.trendModeUsage", "使用")}
      </button>
      <Switch
        checked={mode === "quota"}
        onCheckedChange={(checked) => changeMode(checked ? "quota" : "usage")}
        className="h-5 w-9 data-[state=unchecked]:bg-gray-300 dark:data-[state=unchecked]:bg-gray-700 [&>span]:h-4 [&>span]:w-4 [&>span]:data-[state=checked]:translate-x-4"
        aria-label={t("usage.trendModeToggle", "切换使用/额度趋势")}
      />
      <button
        type="button"
        onClick={() => changeMode("quota")}
        className={cn(
          "transition-colors",
          mode === "quota"
            ? "font-medium text-foreground"
            : "text-muted-foreground hover:text-foreground",
        )}
      >
        {t("usage.trendModeQuota", "额度")}
      </button>
    </div>
  );

  if (mode === "quota") {
    return (
      <QuotaTrendChart
        range={range}
        rangeLabel={rangeLabel}
        appType={appType}
        titleSlot={toggle}
      />
    );
  }

  return (
    <UsageTrendChart
      range={range}
      rangeLabel={rangeLabel}
      appType={appType}
      providerName={providerName}
      model={model}
      refreshIntervalMs={refreshIntervalMs}
      titleSlot={toggle}
    />
  );
}
