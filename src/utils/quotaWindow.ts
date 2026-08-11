/**
 * 订阅窗口时长与「时间%」计算（与托盘 tray.rs / vscode-gptx 语义对齐）。
 *
 * 时间% = 窗口已流逝占比：(窗口长 - 距重置) / 窗口长。
 * 用量 90% 时，时间 10% 与 99% 意义完全不同——这是双百分比展示的核心动机。
 */

const FIVE_HOUR_SECONDS = 5 * 3600;
const SEVEN_DAY_SECONDS = 7 * 24 * 3600;
const THIRTY_DAY_SECONDS = 30 * 24 * 3600;

/** 已知固定窗口的 tier 名 → 秒数 */
const FIXED_WINDOW_SECONDS: Record<string, number> = {
  five_hour: FIVE_HOUR_SECONDS,
  seven_day: SEVEN_DAY_SECONDS,
  seven_day_opus: SEVEN_DAY_SECONDS,
  seven_day_sonnet: SEVEN_DAY_SECONDS,
  weekly_limit: SEVEN_DAY_SECONDS,
  monthly: THIRTY_DAY_SECONDS,
  "30_day": THIRTY_DAY_SECONDS,
};

/** tier 名 → 窗口秒数；未知（如 Gemini 模型档）返回 undefined */
export function tierWindowSeconds(tierName: string): number | undefined {
  if (tierName in FIXED_WINDOW_SECONDS) {
    return FIXED_WINDOW_SECONDS[tierName];
  }
  const hourMatch = /^(\d+)_hour$/.exec(tierName);
  if (hourMatch) {
    const hours = Number(hourMatch[1]);
    return hours > 0 ? hours * 3600 : undefined;
  }
  const dayMatch = /^(\d+)_day$/.exec(tierName);
  if (dayMatch) {
    const days = Number(dayMatch[1]);
    return days > 0 ? days * 24 * 3600 : undefined;
  }
  return undefined;
}

/**
 * 窗口时间流逝百分比 0–100。
 * 缺少重置时间或窗口时长时返回 undefined。
 */
export function elapsedPercent(
  windowSeconds: number,
  resetsAt: string | null | undefined,
  nowMs: number = Date.now(),
): number | undefined {
  if (!resetsAt || windowSeconds <= 0) {
    return undefined;
  }
  const resetMs = Date.parse(resetsAt);
  if (!Number.isFinite(resetMs)) {
    return undefined;
  }
  const windowMs = windowSeconds * 1000;
  const ratio = (windowMs - (resetMs - nowMs)) / windowMs;
  return Math.min(100, Math.max(0, ratio * 100));
}

/** 由 tier 名 + resetsAt 计算时间%；算不出则 undefined */
export function tierElapsedPercent(
  tierName: string,
  resetsAt: string | null | undefined,
  nowMs: number = Date.now(),
): number | undefined {
  const seconds = tierWindowSeconds(tierName);
  if (seconds === undefined) {
    return undefined;
  }
  return elapsedPercent(seconds, resetsAt, nowMs);
}

/**
 * 用量%-时间% 展示文本。
 * - 有时间%：`3%-37%`
 * - 仅用量：`3%`
 */
export function formatUsedElapsedPercent(
  usedPercent: number,
  elapsed: number | undefined,
): string {
  const used = Math.round(usedPercent);
  if (elapsed === undefined || !Number.isFinite(elapsed)) {
    return `${used}%`;
  }
  return `${used}%-${Math.round(elapsed)}%`;
}
