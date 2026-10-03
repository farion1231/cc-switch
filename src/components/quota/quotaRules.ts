import type { TFunction } from "i18next";
import type { QuotaTier } from "@/types/subscription";

/**
 * 额度的文字和颜色（v7 QuotaSpec）：一律写「剩余」，平时灰色；任一档剩余不到 10%（余额不到
 * 总额 10%）加深加粗（不用橙色，见 TONE_TEXT）；用完 / 过期 / 没查到红色。卡片最多两行：
 * 档数更多时，第一行固定写窗口最短的那档，其余并成一行（见 cardRows）。
 */
export type QuotaTone = "normal" | "warning" | "danger" | "muted";

export interface QuotaLine {
  key: string;
  text: string;
  /** 不带档名的值（「剩余 62%」），额度条里档名单独一列 */
  value?: string;
  tone: QuotaTone;
  /** 剩余百分比；余额没有总额时是 Infinity，失败 / 过期是负数（排在最前） */
  left: number;
  /** 悬停时补充的一句（重置时间、套餐名） */
  detail?: string;
  /** 并进卡片合并行时的写法（「每周 64%」）；只有按档的额度行才有 */
  short?: string;
  /** 档位窗口的长短次序，越小越短（见 TIER_WINDOW_ORDER） */
  window?: number;
}

export const WARN_BELOW_PERCENT = 10;

export function toneForLeft(left: number): QuotaTone {
  if (left <= 0) return "danger";
  if (left < WARN_BELOW_PERCENT) return "warning";
  return "normal";
}

/** 中文里档名以字母数字结尾（「每周 Opus」）时，和后面的「剩余」隔一个空格 */
function labelParams(label: string) {
  return {
    label,
    labelSp: /[A-Za-z0-9]$/.test(label) ? `${label} ` : label,
  };
}

/** 计算倒计时的纯时间字符串，如 "2h30m"、"3d12h" */
export function countdownStr(resetsAt: string | null | undefined) {
  if (!resetsAt) return null;
  const diffMs = new Date(resetsAt).getTime() - Date.now();
  if (!Number.isFinite(diffMs) || diffMs <= 0) return null;
  const hours = Math.floor(diffMs / (1000 * 60 * 60));
  const minutes = Math.floor((diffMs % (1000 * 60 * 60)) / (1000 * 60));
  if (hours > 24) return `${Math.floor(hours / 24)}d${hours % 24}h`;
  if (hours > 0) return `${hours}h${minutes}m`;
  return `${minutes}m`;
}

/** 档位窗口的长短：卡片第一行写最短的那档；不认识的档排最后 */
const TIER_WINDOW_ORDER: Record<string, number> = {
  five_hour: 0,
  gemini_pro: 1,
  gemini_flash: 1,
  gemini_flash_lite: 1,
  seven_day: 2,
  seven_day_fable: 2,
  seven_day_opus: 2,
  seven_day_sonnet: 2,
  weekly_limit: 2,
  "30_day": 3,
  monthly: 3,
  credits: 3,
  premium: 3,
};
const UNKNOWN_WINDOW = 9;

/** `shortLabel` 给了才能并进卡片的合并行（英日用短档名，放得下 136px 那一列） */
export function tierLine(
  t: TFunction,
  tier: Pick<QuotaTier, "name" | "utilization" | "resetsAt">,
  label: string,
  shortLabel?: string,
): QuotaLine {
  const left = Math.max(0, Math.round(100 - (tier.utilization ?? 0)));
  const params = labelParams(label);
  const countdown = countdownStr(tier.resetsAt);
  return {
    key: tier.name,
    left,
    tone: toneForLeft(left),
    text:
      left <= 0
        ? t("quota.tierUsedUp", params)
        : t("quota.tierLeft", { ...params, value: left }),
    value: left <= 0 ? t("quota.usedUp") : t("quota.left", { value: left }),
    detail: countdown
      ? `${label} · ${t("subscription.resetsIn", { time: countdown })}`
      : undefined,
    short:
      shortLabel === undefined
        ? undefined
        : t("quota.tierShort", { label: shortLabel, value: left }),
    window: TIER_WINDOW_ORDER[tier.name] ?? UNKNOWN_WINDOW,
  };
}

export function balanceLine(
  t: TFunction,
  {
    key = "balance",
    remaining,
    total,
    unit,
    detail,
  }: {
    key?: string;
    remaining: number;
    total?: number | null;
    unit?: string | null;
    detail?: string;
  },
): QuotaLine {
  const hasTotal = typeof total === "number" && total > 0;
  const left =
    remaining <= 0 ? 0 : hasTotal ? (remaining / total) * 100 : Infinity;
  const value = `${remaining.toFixed(2)}${unit ? ` ${unit}` : ""}`;
  return {
    key,
    left,
    tone: toneForLeft(left),
    text:
      remaining <= 0 ? t("quota.balanceUsedUp") : t("quota.balance", { value }),
    detail,
  };
}

export function expiredLine(
  t: TFunction,
  detail?: string,
  key = "expired",
): QuotaLine {
  return {
    key,
    left: -1,
    tone: "danger",
    text: t("quota.planExpired"),
    detail,
  };
}

/** 查询失败：第一行红字，第二行灰字写原因 */
export function failedLines(t: TFunction, reason?: string | null): QuotaLine[] {
  const lines: QuotaLine[] = [
    { key: "failed", left: -2, tone: "danger", text: t("quota.failed") },
  ];
  const text = reason?.trim();
  if (text) lines.push({ key: "reason", left: -2, tone: "muted", text });
  return lines;
}

/** 卡片上最多留几行：留剩余最少的，再按原顺序排回去 */
export function pickLines(lines: QuotaLine[], max = 2): QuotaLine[] {
  if (lines.length <= max) return lines;
  return lines
    .map((line, index) => ({ line, index }))
    .sort((a, b) => a.line.left - b.line.left)
    .slice(0, max)
    .sort((a, b) => a.index - b.index)
    .map(({ line }) => line);
}

/** 合并行最多几段，再多就放不下了 */
const MERGED_MAX = 2;

/**
 * 卡片上的各行，每行一段或几段。放得下就一档一行；放不下时，按档的额度第一行固定写窗口
 * 最短的那档（位置不随用量跳），其余并成一行（多于两段留剩余最少的）；其他额度行
 * （余额、失败）照旧留剩余最少的几行
 */
export function cardRows(lines: QuotaLine[], max = 2): QuotaLine[][] {
  if (lines.length <= max) return lines.map((line) => [line]);
  if (max < 2 || !lines.every((line) => line.short)) {
    return pickLines(lines, max).map((line) => [line]);
  }
  const heads = lines
    .map((line, index) => ({ line, index }))
    .sort(
      (a, b) =>
        (a.line.window ?? UNKNOWN_WINDOW) - (b.line.window ?? UNKNOWN_WINDOW) ||
        a.index - b.index,
    )
    .slice(0, max - 1)
    .map(({ line }) => line);
  const rest = lines.filter((line) => !heads.includes(line));
  return [...heads.map((line) => [line]), pickLines(rest, MERGED_MAX)];
}

/** 相对时间（「3 分钟前」） */
export function formatRelativeTime(
  timestamp: number,
  now: number,
  t: TFunction,
): string {
  const diff = Math.floor((now - timestamp) / 1000);
  if (diff < 60) return t("usage.justNow");
  if (diff < 3600)
    return t("usage.minutesAgo", { count: Math.floor(diff / 60) });
  if (diff < 86400)
    return t("usage.hoursAgo", { count: Math.floor(diff / 3600) });
  return t("usage.daysAgo", { count: Math.floor(diff / 86400) });
}
