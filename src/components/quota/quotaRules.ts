import type { TFunction } from "i18next";
import type { QuotaTier, ResetCredits } from "@/types/subscription";

/**
 * 额度的文字和颜色（v7 QuotaSpec）：一律写「剩余」，平时灰色；任一档剩余不到 10% 加深加粗
 * （不用橙色，见 TONE_TEXT；余额不算，见 balanceLine）；用完 / 过期 / 没查到红色。卡片最多两行：
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
  /** 悬停时补充的一句（套餐名、失败原因）；重置时间不写这里，见 resetsAt */
  detail?: string;
  /** 档名（「5 小时」），悬停说明里接在重置倒计时前面 */
  label?: string;
  /** 这档下次重置的时间；倒计时在渲染时按当前时间现算（resetText / lineHint） */
  resetsAt?: string | null;
  /** 并进卡片合并行时的写法（「每周 64%」）；只有按档的额度行才有 */
  short?: string;
  /** 档位窗口的长短次序，越小越短（见 TIER_WINDOW_ORDER） */
  window?: number;
  /** 没有比例可画时，额度条的位置改写这句（重置次数写最早的到期日） */
  caption?: string;
  /** 一行写不下的明细，点开额度行时逐条列出（重置次数按到期日分组） */
  breakdown?: QuotaBreakdown;
}

export interface QuotaBreakdown {
  title: string;
  /** 点开按钮的无障碍名字 */
  openLabel: string;
  items: QuotaBreakdownItem[];
}

export interface QuotaBreakdownItem {
  key: string;
  /** 「10月12日」/「不会过期」 */
  label: string;
  /** 「8d3h后」 */
  hint?: string;
  /** 「2 次」 */
  value: string;
  tone: QuotaTone;
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
export function countdownStr(
  resetsAt: string | null | undefined,
  now = Date.now(),
) {
  if (!resetsAt) return null;
  const diffMs = new Date(resetsAt).getTime() - now;
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
  return {
    key: tier.name,
    left,
    tone: toneForLeft(left),
    text:
      left <= 0
        ? t("quota.tierUsedUp", params)
        : t("quota.tierLeft", { ...params, value: left }),
    value: left <= 0 ? t("quota.usedUp") : t("quota.left", { value: left }),
    label,
    resetsAt: tier.resetsAt,
    short:
      shortLabel === undefined
        ? undefined
        : t("quota.tierShort", { label: shortLabel, value: left }),
    window: TIER_WINDOW_ORDER[tier.name] ?? UNKNOWN_WINDOW,
  };
}

/** 「2h30m后重置」；没有重置时间或已经过了时为 null */
export function resetText(
  t: TFunction,
  line: Pick<QuotaLine, "resetsAt">,
  now = Date.now(),
): string | null {
  const countdown = countdownStr(line.resetsAt, now);
  return countdown ? t("subscription.resetsIn", { time: countdown }) : null;
}

/** 一行额度的悬停说明：补充说明 + 「档名 · x 后重置」；两样都没有时就是这行本身 */
export function lineHint(t: TFunction, line: QuotaLine, now = Date.now()) {
  const reset = resetText(t, line, now);
  return (
    [line.detail, reset && (line.label ? `${line.label} · ${reset}` : reset)]
      .filter(Boolean)
      .join(" · ") || line.text
  );
}

/** 最早那次重置三天内就过期时加深提醒 */
export const RESET_EXPIRING_SOON_MS = 3 * 24 * 60 * 60 * 1000;

/** 排在所有档位之后：卡片合并时跟在每周那档后面（「每周 64% · 重置 1 次」） */
const RESET_CREDITS_WINDOW = UNKNOWN_WINDOW + 1;

function shortDate(iso: string, locale: string): string {
  const date = new Date(iso);
  const sameYear = date.getFullYear() === new Date().getFullYear();
  try {
    return new Intl.DateTimeFormat(
      locale,
      sameYear
        ? { month: "short", day: "numeric" }
        : { year: "numeric", month: "short", day: "numeric" },
    ).format(date);
  } catch {
    return date.toLocaleDateString();
  }
}

/**
 * ChatGPT 订阅存下的限额重置 → 额度行；一次都没有时不显示（null）。
 * 查询之后才过期的也在这里去掉（额度会缓存一阵）。
 */
export function resetCreditsLine(
  t: TFunction,
  credits: ResetCredits | null | undefined,
  { now = Date.now(), locale }: { now?: number; locale: string },
): QuotaLine | null {
  const expiries = (credits?.expiresAt ?? []).filter((at) => {
    if (!at) return true;
    const ms = Date.parse(at);
    return !Number.isFinite(ms) || ms > now;
  });
  const count = expiries.length;
  if (count === 0) return null;

  // 后端已按到期先后排好，不过期的在最后
  const first = expiries[0];
  const firstMs = first ? Date.parse(first) : NaN;
  const date =
    first && Number.isFinite(firstMs) ? shortDate(first, locale) : null;
  const expiringSoon =
    Number.isFinite(firstMs) && firstMs - now < RESET_EXPIRING_SOON_MS;

  return {
    key: "reset_credits",
    left: Infinity,
    tone: expiringSoon ? "warning" : "normal",
    text: t("quota.resetCredits.left", { count }),
    value: t("quota.resetCredits.value", { count }),
    short: t("quota.resetCredits.short", { count }),
    caption: date
      ? t("quota.resetCredits.expiresOn", { date })
      : t("quota.resetCredits.noExpiry"),
    detail: date
      ? t("quota.resetCredits.detail", { count, date })
      : t("quota.resetCredits.detailNoExpiry", { count }),
    window: RESET_CREDITS_WINDOW,
    // 只有一次时行里已经写全了，不用再点开
    breakdown:
      count > 1
        ? {
            title: t("quota.resetCredits.title"),
            openLabel: t("quota.resetCredits.showAll", { count }),
            items: resetCreditGroups(t, expiries, { now, locale }),
          }
        : undefined,
  };
}

/** 同一天到期的并成一条（「10月12日 · 8d3h后 · 2 次」），不过期的排最后 */
function resetCreditGroups(
  t: TFunction,
  expiries: (string | null)[],
  { now, locale }: { now: number; locale: string },
): QuotaBreakdownItem[] {
  // 解析不出的到期时间和后端一样当作不过期
  const groups = new Map<string, { at: string | null; count: number }>();
  for (const at of expiries) {
    const known = at && Number.isFinite(Date.parse(at)) ? at : null;
    const key = known ? shortDate(known, locale) : "no_expiry";
    const group = groups.get(key);
    if (group) group.count += 1;
    else groups.set(key, { at: known, count: 1 });
  }
  return [...groups].map(([key, { at, count }]) => {
    const countdown = countdownStr(at, now);
    return {
      key,
      label: at ? key : t("quota.resetCredits.noExpiry"),
      hint: countdown
        ? t("quota.resetCredits.inTime", { time: countdown })
        : undefined,
      value: t("quota.resetCredits.times", { count }),
      tone:
        at && Date.parse(at) - now < RESET_EXPIRING_SOON_MS
          ? "warning"
          : "normal",
    };
  });
}

/**
 * 余额一律灰色，只有用完才变红：不做「快用完」的加深。几张卡片的余额深浅不一，
 * 读起来像出了什么错，而不是「快用完了」（10-04 Jason 定）。left 照旧按总额算，
 * 展开时的条长、多行时挑哪几行还用它
 */
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
    tone: remaining <= 0 ? "danger" : "normal",
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

/** 查询失败那一行的 key：额度列靠它判断这次重查有没有成功 */
export const FAILED_LINE_KEY = "failed";

/** 查询失败：第一行红字，第二行灰字写原因 */
export function failedLines(t: TFunction, reason?: string | null): QuotaLine[] {
  const lines: QuotaLine[] = [
    {
      key: FAILED_LINE_KEY,
      left: -2,
      tone: "danger",
      text: t("quota.failed"),
    },
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
