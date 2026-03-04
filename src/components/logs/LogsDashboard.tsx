import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { usageKeys } from "@/lib/query/usage";
import type { TimeRange } from "@/types/usage";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { useIsFetching, useQueryClient } from "@tanstack/react-query";
import { motion } from "framer-motion";
import { ChevronDown, RefreshCw } from "lucide-react";
import { useMemo, useState, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { LogList } from "./LogList";

interface LogsDashboardProps {}

// Get initial time range from URL or default
const getInitialTimeRange = (): TimeRange => {
  const params = new URLSearchParams(window.location.search);
  const timeRange = params.get("timeRange");
  const validRanges: TimeRange[] = ["5m", "15m", "30m", "1h", "5h", "12h", "1d", "7d", "30d"];
  if (timeRange && validRanges.includes(timeRange as TimeRange)) {
    return timeRange as TimeRange;
  }
  return "5m";
};

export function LogsDashboard({}: LogsDashboardProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const isFetching = useIsFetching({ queryKey: usageKeys.all });
  const [refreshIntervalMs, setRefreshIntervalMs] = useState(0);
  const [timeRange, setTimeRange] = useState<TimeRange>(getInitialTimeRange);
  const [showRefreshMenu, setShowRefreshMenu] = useState(false);
  const [showTimeMenu, setShowTimeMenu] = useState(false);
  const [manualRefreshTrigger, setManualRefreshTrigger] = useState(0);

  // Quick time options
  const QUICK_TIME_OPTIONS = [
    { value: "5m", label: () => t("logs.timeOptions.5m") },
    { value: "15m", label: () => t("logs.timeOptions.15m") },
    { value: "30m", label: () => t("logs.timeOptions.30m") },
  ];

  // More time options (dropdown)
  const MORE_TIME_OPTIONS = [
    { value: "1h", label: () => t("logs.timeOptions.1h") },
    { value: "5h", label: () => t("logs.timeOptions.5h") },
    { value: "12h", label: () => t("logs.timeOptions.12h") },
    { value: "1d", label: () => t("logs.timeOptions.1d") },
    { value: "7d", label: () => t("logs.timeOptions.7d") },
    { value: "30d", label: () => t("logs.timeOptions.30d") },
  ];

  // Refresh interval options
  const REFRESH_OPTIONS = [
    { value: 0, label: () => t("logs.refreshOptions.0") },
    { value: 1000, label: () => t("logs.refreshOptions.1000") },
    { value: 5000, label: () => t("logs.refreshOptions.5000") },
    { value: 10000, label: () => t("logs.refreshOptions.10000") },
    { value: 15000, label: () => t("logs.refreshOptions.15000") },
    { value: 30000, label: () => t("logs.refreshOptions.30000") },
    { value: 60000, label: () => t("logs.refreshOptions.60000") },
  ];

  const changeRefreshInterval = (value: number) => {
    setRefreshIntervalMs(value);
    setShowRefreshMenu(false);
  };

  const handleManualRefresh = () => {
    // Clear all usage-related query cache
    queryClient.removeQueries({ queryKey: ["usage"], exact: false });
    // Then trigger remount
    setManualRefreshTrigger((t) => t + 1);
  };

  // Auto refresh: handle at LogsDashboard level to ensure time range also updates
  const refreshIntervalRef = useRef<number | undefined>(undefined);
  useEffect(() => {
    // If interval hasn't changed, do nothing
    if (refreshIntervalMs === refreshIntervalRef.current) {
      return;
    }
    refreshIntervalRef.current = refreshIntervalMs;

    // Clear previous timer
    if (
      refreshIntervalRef.current !== undefined &&
      refreshIntervalRef.current > 0
    ) {
      const timer = setInterval(() => {
        setManualRefreshTrigger((t) => t + 1);
      }, refreshIntervalRef.current);
      return () => {
        clearInterval(timer);
      };
    }
  }, [refreshIntervalMs]);

  // Calculate time range in seconds
  const getTimeRangeSeconds = (range: TimeRange): number => {
    switch (range) {
      case "5m":
        return 5 * 60;
      case "15m":
        return 15 * 60;
      case "30m":
        return 30 * 60;
      case "1h":
        return 60 * 60;
      case "5h":
        return 5 * 60 * 60;
      case "12h":
        return 12 * 60 * 60;
      case "1d":
        return 24 * 60 * 60;
      case "7d":
        return 7 * 24 * 60 * 60;
      case "30d":
        return 30 * 24 * 60 * 60;
      default:
        return 5 * 60;
    }
  };

  // Calculate time range - recalculate on each render to ensure window is "now"
  const timeRangeParams = useMemo(
    () => ({
      startDate: Math.floor(Date.now() / 1000) - getTimeRangeSeconds(timeRange),
      endDate: Math.floor(Date.now() / 1000),
    }),
    [timeRange, manualRefreshTrigger, refreshIntervalMs],
  ); // Add dependency to ensure refresh updates

  // Get current refresh option label
  const getCurrentRefreshLabel = () => {
    const option = REFRESH_OPTIONS.find((o) => o.value === refreshIntervalMs);
    return option ? option.label() : t("logs.refreshOptions.0");
  };

  // Get current time option label
  const getCurrentTimeLabel = () => {
    const quick = QUICK_TIME_OPTIONS.find((o) => o.value === timeRange);
    if (quick) return quick.label();
    const more = MORE_TIME_OPTIONS.find((o) => o.value === timeRange);
    return more ? more.label() : t("logs.timeOptions.5m");
  };

  // Check if current time is in quick options
  const isQuickTime = QUICK_TIME_OPTIONS.some((o) => o.value === timeRange);

  // Update URL when timeRange changes
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    params.set("timeRange", timeRange);
    const newUrl = `${window.location.pathname}?${params.toString()}`;
    window.history.replaceState({}, "", newUrl);
  }, [timeRange]);

  return (
    <div className="flex flex-col h-full">
      {/* Control bar - fixed */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 shrink-0 overflow-visible">
        <div className="flex flex-col gap-1">
          <h2 className="text-2xl font-bold">{t("logs.title")}</h2>
          <p className="text-sm text-muted-foreground whitespace-nowrap">{t("logs.subtitle")}</p>
        </div>
        <div className="flex flex-wrap items-end justify-end gap-2 w-full overflow-visible pb-2 sm:pb-0">
          {/* Time filter box */}
          <div className="flex items-center h-10 shrink-0">
            {/* Time filter Tabs */}
            <Tabs
              value={isQuickTime ? timeRange : "custom"}
              onValueChange={(v) => {
                if (v !== "custom") setTimeRange(v as TimeRange);
              }}
            >
              <TabsList className="h-10 p-1 bg-card/60 border border-border/50 backdrop-blur-sm flex items-center">
                {QUICK_TIME_OPTIONS.map((option) => (
                  <TabsTrigger
                    key={option.value}
                    value={option.value}
                    className="text-xs px-4 h-8 data-[state=active]:bg-primary/10 data-[state=active]:text-primary"
                  >
                    {option.label()}
                  </TabsTrigger>
                ))}

                {/* Divider */}
                <div className="w-px h-6 bg-border mx-1" />

                {/* More time dropdown */}
                <DropdownMenu.Root
                  open={showTimeMenu}
                  onOpenChange={setShowTimeMenu}
                >
                  <DropdownMenu.Trigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className={`h-8 px-4 text-xs ${!isQuickTime ? "text-primary bg-primary/10" : ""}`}
                    >
                      {getCurrentTimeLabel()}
                      <ChevronDown className="h-3 w-3 ml-1" />
                    </Button>
                  </DropdownMenu.Trigger>
                  <DropdownMenu.Portal>
                    <DropdownMenu.Content
                      className="z-[9999] bg-background border rounded-lg shadow-lg py-1 min-w-[120px]"
                      align="end"
                    >
                      {[...QUICK_TIME_OPTIONS, ...MORE_TIME_OPTIONS].map(
                        (option) => (
                          <DropdownMenu.Item
                            key={option.value}
                            className={`cursor-pointer px-3 py-2 text-sm hover:bg-accent ${
                              timeRange === option.value
                                ? "bg-accent font-medium"
                                : ""
                            }`}
                            onClick={() => {
                              setTimeRange(option.value as TimeRange);
                              setShowTimeMenu(false);
                            }}
                          >
                            {option.label()}
                          </DropdownMenu.Item>
                        ),
                      )}
                    </DropdownMenu.Content>
                  </DropdownMenu.Portal>
                </DropdownMenu.Root>
              </TabsList>
            </Tabs>
          </div>

          {/* Refresh button box */}
          <Tabs value="refresh" className="flex">
            <TabsList className="h-10 p-1 bg-card/60 border border-border/50 backdrop-blur-sm flex items-center min-w-[140px]">
              {/* Manual refresh button */}
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-8 px-4 text-xs"
                title={t("common.refresh")}
                onClick={handleManualRefresh}
              >
                <RefreshCw
                  className={`h-3.5 w-3.5 mr-1 ${isFetching > 0 ? "animate-spin" : ""}`}
                />
                {t("common.refresh")}
              </Button>

              {/* Divider */}
              <div className="w-px h-6 bg-border mx-1" />

              {/* Refresh interval dropdown */}
              <DropdownMenu.Root
                open={showRefreshMenu}
                onOpenChange={setShowRefreshMenu}
              >
                <DropdownMenu.Trigger asChild>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="h-8 px-4 text-xs"
                  >
                    {getCurrentRefreshLabel()}
                    <ChevronDown className="h-3 w-3 ml-1" />
                  </Button>
                </DropdownMenu.Trigger>
                <DropdownMenu.Portal>
                  <DropdownMenu.Content
                    className="z-[9999] bg-background border rounded-lg shadow-lg py-1 min-w-[120px]"
                    align="end"
                  >
                    {REFRESH_OPTIONS.map((option) => (
                      <DropdownMenu.Item
                        key={option.value}
                        className={`cursor-pointer px-3 py-2 text-sm hover:bg-accent ${
                          refreshIntervalMs === option.value
                            ? "bg-accent font-medium"
                            : ""
                        }`}
                        onClick={() => changeRefreshInterval(option.value)}
                      >
                        {option.label()}
                      </DropdownMenu.Item>
                    ))}
                  </DropdownMenu.Content>
                </DropdownMenu.Portal>
              </DropdownMenu.Root>
            </TabsList>
          </Tabs>
        </div>
      </div>

      <motion.div
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.1 }}
        className="flex-1 min-h-0 mt-2"
      >
        <LogList
          key={manualRefreshTrigger}
          startDate={timeRangeParams.startDate}
          endDate={timeRangeParams.endDate}
          refreshIntervalMs={refreshIntervalMs}
          manualRefreshTrigger={manualRefreshTrigger}
        />
      </motion.div>
    </div>
  );
}
