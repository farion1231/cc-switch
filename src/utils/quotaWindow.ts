/**
 * 订阅窗口时长与「时间%」计算（与托盘 tray.rs / vscode-gptx 语义对齐）。
 *
 * 时间% = 窗口已流逝占比：(窗口长 - 距重置) / 窗口长。
 * 用量 90% 时，时间 10% 与 99% 意义完全不同——这是双百分比展示的核心动机。
 *
 * 「超进度」：用量% > 时间%（烧得比时间快），UI 对用量数字加粗提醒。
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

/** 末尾已带时区标记（`Z` 或 `±HH:MM` / `±HHMM`） */
const HAS_TZ_OFFSET = /(?:Z|[+-]\d{2}:?\d{2})$/i;

/** 无时区的日期时间：`2026-08-12T12:00:00(.123)` 或空格分隔变体 */
const NAIVE_DATETIME =
  /^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?$/;

/**
 * 解析重置时间戳，与 tray.rs `parse_resets_at_ms` 语义一致：
 * 不带时区的形式按 **UTC** 解释，而非 WebView 本地时区。
 * 裸 `Date.parse("2026-08-12T12:00:00")` 会当成本地时间，东八区就会偏 8 小时，
 * 对 5 小时窗口直接被 clamp 成 0% / 100%，且与托盘显示不一致。
 */
export function parseResetsAtMs(resetsAt: string): number {
  const trimmed = resetsAt.trim();
  if (!HAS_TZ_OFFSET.test(trimmed) && NAIVE_DATETIME.test(trimmed)) {
    return Date.parse(`${trimmed.replace(" ", "T")}Z`);
  }
  return Date.parse(trimmed);
}

/**
 * 允许「距重置」略超窗口长的容差（刚重置 + 客户端时钟偏差）。
 * 与 tray.rs `WINDOW_OVERSHOOT_TOLERANCE` 保持一致。
 */
const WINDOW_OVERSHOOT_TOLERANCE = 0.05;

/**
 * 窗口时间流逝百分比 0–100。
 * 缺少重置时间或窗口时长时返回 undefined。
 *
 * 若「距重置」明显超过窗口长，说明该 tier 的窗口时长只是启发式猜测
 * （如 Grok 按重置距离反推的 `weekly_limit` / `monthly`，见
 * `subscription_grok::tier_name_for_reset`），此时返回 undefined 退化为只显示
 * 用量%——而不是 clamp 成 0% 后把任何非零用量都误判成「超进度」。
 */
export function elapsedPercent(
  windowSeconds: number,
  resetsAt: string | null | undefined,
  nowMs: number = Date.now(),
): number | undefined {
  if (!resetsAt || windowSeconds <= 0) {
    return undefined;
  }
  const resetMs = parseResetsAtMs(resetsAt);
  if (!Number.isFinite(resetMs)) {
    return undefined;
  }
  const windowMs = windowSeconds * 1000;
  const remainingMs = resetMs - nowMs;
  if (remainingMs > windowMs * (1 + WINDOW_OVERSHOOT_TOLERANCE)) {
    return undefined;
  }
  const ratio = (windowMs - remainingMs) / windowMs;
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
 * 用量是否跑赢时间进度（烧得比时间快）。
 * 与 vscode-gptx `isOverPace` 一致：两边都有值且 used > elapsed。
 */
export function isOverPace(
  usedPercent: number,
  elapsedPercent: number | undefined,
): boolean {
  return (
    elapsedPercent !== undefined &&
    Number.isFinite(elapsedPercent) &&
    Number.isFinite(usedPercent) &&
    usedPercent > elapsedPercent
  );
}

export interface UsedElapsedParts {
  /** 圆整后的用量数字文案，如 "39%" */
  usedText: string;
  /** 圆整后的时间数字文案，如 "70%"；无时间% 时为 undefined */
  elapsedText?: string;
  /** 纯文本：`39%-70%` 或 `39%` */
  plain: string;
  /** 用量是否超过时间进度，应用加粗 */
  overPace: boolean;
}

/** 拆成可分别加粗的用量/时间片段，供 React 渲染 */
export function splitUsedElapsedPercent(
  usedPercent: number,
  elapsed: number | undefined,
): UsedElapsedParts {
  const usedText = `${Math.round(usedPercent)}%`;
  if (elapsed === undefined || !Number.isFinite(elapsed)) {
    return {
      usedText,
      plain: usedText,
      overPace: false,
    };
  }
  const elapsedText = `${Math.round(elapsed)}%`;
  return {
    usedText,
    elapsedText,
    plain: `${usedText}-${elapsedText}`,
    overPace: isOverPace(usedPercent, elapsed),
  };
}

/**
 * 用量%-时间% 纯文本。
 * - 有时间%：`3%-37%`
 * - 仅用量：`3%`
 */
export function formatUsedElapsedPercent(
  usedPercent: number,
  elapsed: number | undefined,
): string {
  return splitUsedElapsedPercent(usedPercent, elapsed).plain;
}
