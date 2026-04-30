import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { UsageSummaryCards } from "./UsageSummaryCards";
import { UsageTrendChart } from "./UsageTrendChart";
import { RequestLogTable } from "./RequestLogTable";
import { ProviderStatsTable } from "./ProviderStatsTable";
import { ModelStatsTable } from "./ModelStatsTable";
import type { AppTypeFilter, UsageRangeSelection } from "@/types/usage";
import { motion } from "framer-motion";
import {
  BarChart3,
  Activity,
  RefreshCw,
  Coins,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useQueryClient } from "@tanstack/react-query";
import { usageKeys } from "@/lib/query/usage";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { PricingConfigPanel } from "@/components/usage/PricingConfigPanel";
import { cn } from "@/lib/utils";
import { getLocaleFromLanguage } from "./format";
import { getUsageRangePresetLabel, resolveUsageRange } from "@/lib/usageRange";
import { UsageDateRangePicker } from "./UsageDateRangePicker";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";

const APP_FILTER_OPTIONS: AppTypeFilter[] = [
  "all",
  "claude",
  "codex",
  "gemini",
  "hermes",
];

const TIME_PRESETS: { key: UsageRangeSelection["preset"]; label: string }[] = [
  { key: "today", label: "Today" },
  { key: "7d", label: "7D" },
  { key: "14d", label: "14D" },
  { key: "30d", label: "30D" },
];

export function UsageDashboard() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const [range, setRange] = useState<UsageRangeSelection>({ preset: "30d" });
  const [appType, setAppType] = useState<AppTypeFilter>("all");
  const [activeTab, setActiveTab] = useState<"overview" | "models">("overview");
  const [refreshIntervalMs, setRefreshIntervalMs] = useState(30000);

  const refreshIntervalOptionsMs = [0, 5000, 10000, 30000, 60000] as const;
  const changeRefreshInterval = () => {
    const currentIndex = refreshIntervalOptionsMs.indexOf(
      refreshIntervalMs as (typeof refreshIntervalOptionsMs)[number],
    );
    const safeIndex = currentIndex >= 0 ? currentIndex : 3;
    const nextIndex = (safeIndex + 1) % refreshIntervalOptionsMs.length;
    const next = refreshIntervalOptionsMs[nextIndex];
    setRefreshIntervalMs(next);
    queryClient.invalidateQueries({ queryKey: usageKeys.all });
  };

  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);
  const resolvedRange = useMemo(() => resolveUsageRange(range), [range]);
  const rangeLabel = useMemo(() => {
    if (range.preset !== "custom") {
      return getUsageRangePresetLabel(range.preset, t);
    }
    return `${new Date(resolvedRange.startDate * 1000).toLocaleString(locale)} - ${new Date(
      resolvedRange.endDate * 1000,
    ).toLocaleString(locale)}`;
  }, [locale, range, resolvedRange.endDate, resolvedRange.startDate, t]);

  return (
    <motion.div
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ type: "spring", stiffness: 500, damping: 35, mass: 0.6 }}
      className="space-y-6 pb-8"
    >
      {/* Top bar: Tabs left, time filters right */}
      <div className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-4">
          <Tabs
            value={activeTab}
            onValueChange={(v) => setActiveTab(v as "overview" | "models")}
          >
            <TabsList>
              <TabsTrigger value="overview" className="gap-1.5">
                <BarChart3 className="h-3.5 w-3.5" />
                {t("usage.overview", "Overview")}
              </TabsTrigger>
              <TabsTrigger value="models" className="gap-1.5">
                <Activity className="h-3.5 w-3.5" />
                {t("usage.models", "Models")}
              </TabsTrigger>
            </TabsList>
          </Tabs>

          {/* App filter pills */}
          <div className="flex items-center gap-1">
            {APP_FILTER_OPTIONS.map((type) => (
              <button
                key={type}
                type="button"
                onClick={() => setAppType(type)}
                className={cn(
                  "px-3 py-1 rounded-lg text-xs font-medium transition-all duration-200",
                  appType === type
                    ? "liquid-glass-subtle text-foreground"
                    : "text-muted-foreground/50 hover:text-muted-foreground hover:bg-white/20 dark:hover:bg-white/5",
                )}
              >
                {t(`usage.appFilter.${type}`)}
              </button>
            ))}
          </div>
        </div>

        {/* Right side: time presets + refresh */}
        <div className="flex items-center gap-1.5">
          {TIME_PRESETS.map((preset) => (
            <button
              key={preset.key}
              type="button"
              onClick={() => setRange({ preset: preset.key })}
              className={cn(
                "px-3 py-1 rounded-lg text-xs font-medium transition-all duration-200",
                range.preset === preset.key
                  ? "liquid-glass-subtle text-foreground"
                  : "text-muted-foreground/50 hover:text-muted-foreground hover:bg-white/20 dark:hover:bg-white/5",
              )}
            >
              {preset.label}
            </button>
          ))}

          <UsageDateRangePicker
            selection={range}
            triggerLabel={range.preset === "custom" ? rangeLabel : "Custom"}
            onApply={(nextRange) => setRange(nextRange)}
          />

          <div className="w-px h-5 bg-border/30 mx-1" />

          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 px-2 text-xs text-muted-foreground/60"
            title={t("common.refresh", "刷新")}
            onClick={changeRefreshInterval}
          >
            <RefreshCw className="h-3 w-3" />
          </Button>
        </div>
      </div>

      {/* Content based on active tab */}
      {activeTab === "overview" ? (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.2 }}
          className="space-y-6"
        >
          {/* Summary cards */}
          <UsageSummaryCards
            range={range}
            appType={appType}
            refreshIntervalMs={refreshIntervalMs}
          />

          {/* Stacked bar chart */}
          <UsageTrendChart
            range={range}
            rangeLabel={rangeLabel}
            appType={appType}
            refreshIntervalMs={refreshIntervalMs}
          />

          {/* Request logs table */}
          <RequestLogTable
            range={range}
            rangeLabel={rangeLabel}
            appType={appType}
            refreshIntervalMs={refreshIntervalMs}
            onRangeChange={setRange}
          />
        </motion.div>
      ) : (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.2 }}
          className="space-y-6"
        >
          {/* Model stats with visual bars */}
          <ModelStatsTable
            range={range}
            appType={appType}
            refreshIntervalMs={refreshIntervalMs}
          />

          {/* Provider stats */}
          <ProviderStatsTable
            range={range}
            appType={appType}
            refreshIntervalMs={refreshIntervalMs}
          />
        </motion.div>
      )}

      {/* Pricing config (always visible, collapsed) */}
      <Accordion type="multiple" defaultValue={[]} className="w-full">
        <AccordionItem value="pricing" className="liquid-glass rounded-2xl overflow-hidden">
          <AccordionTrigger className="px-5 py-4 hover:no-underline hover:bg-white/20 dark:hover:bg-white/5 data-[state=open]:bg-white/20 dark:data-[state=open]:bg-white/5">
            <div className="flex items-center gap-3">
              <Coins className="h-4 w-4 text-amber-500" />
              <div className="text-left">
                <h3 className="text-sm font-semibold">
                  {t("settings.advanced.pricing.title")}
                </h3>
                <p className="text-xs text-muted-foreground font-normal">
                  {t("settings.advanced.pricing.description")}
                </p>
              </div>
            </div>
          </AccordionTrigger>
          <AccordionContent className="px-5 pb-5 pt-3 border-t border-white/10 dark:border-white/5">
            <PricingConfigPanel />
          </AccordionContent>
        </AccordionItem>
      </Accordion>
    </motion.div>
  );
}
