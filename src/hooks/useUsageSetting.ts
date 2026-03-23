import { useState, useEffect } from "react";
import type { TimeRange } from "@/types/usage";

const TIME_RANGE_KEY = "cc-switch-usage-time-range";
const REFRESH_INTERVAL_KEY = "cc-switch-usage-refresh-interval";

const DEFAULT_TIME_RANGE: TimeRange = "1d";
const DEFAULT_REFRESH_INTERVAL = 30000;

function isValidTimeRange(value: string | null): value is TimeRange {
  return value === "5h" || value === "1d" || value === "7d" || value === "30d";
}

function isValidRefreshInterval(value: number | null): value is number {
  return value !== null && [0, 5000, 10000, 30000, 60000].includes(value);
}

export function useUsageSetting() {
  // 时间窗口
  const [timeRange, setTimeRangeState] = useState<TimeRange>(() => {
    if (typeof window === "undefined") return DEFAULT_TIME_RANGE;

    const stored = window.localStorage.getItem(TIME_RANGE_KEY);
    return isValidTimeRange(stored) ? stored : DEFAULT_TIME_RANGE;
  });

  useEffect(() => {
    if (typeof window === "undefined") return;
    window.localStorage.setItem(TIME_RANGE_KEY, timeRange);
  }, [timeRange]);

  // 刷新间隔
  const [refreshIntervalMs, setRefreshIntervalMsState] = useState<number>(() => {
    if (typeof window === "undefined") return DEFAULT_REFRESH_INTERVAL;

    const stored = window.localStorage.getItem(REFRESH_INTERVAL_KEY);
    const parsed = stored ? parseInt(stored, 10) : null;
    return isValidRefreshInterval(parsed) ? parsed : DEFAULT_REFRESH_INTERVAL;
  });

  useEffect(() => {
    if (typeof window === "undefined") return;
    window.localStorage.setItem(REFRESH_INTERVAL_KEY, String(refreshIntervalMs));
  }, [refreshIntervalMs]);

  return {
    timeRange,
    setTimeRange: setTimeRangeState,
    refreshIntervalMs,
    setRefreshIntervalMs: setRefreshIntervalMsState,
  };
}
