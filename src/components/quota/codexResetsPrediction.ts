import type { TFunction } from "i18next";
import type { CodexResetsStatus } from "@/types/subscription";
import { countdownStr, formatRelativeTime } from "./quotaRules";

/**
 * codex-resets.com 重置预测的行摘要与悬浮详情（纯逻辑，便于测试）。
 * 时间一律原样给 ISO 字符串，格式化（相对时间 / 倒计时 / 绝对时刻）由
 * detailText 统一做；主窗口卡片和托盘面板共用。
 */

export type CodexResetsSummary =
  /** 进行中的重置观测（expiresAt 未过才算数） */
  | { kind: "watch"; chancePercent: number | null }
  /** 已公告、预定时刻还在未来的常规重置 */
  | { kind: "scheduled"; resetsAt: string }
  /** 既没有观测也没有待执行的重置公告 */
  | { kind: "noSignal" };

export function summarizeCodexResets(
  status: CodexResetsStatus,
  now = Date.now(),
): CodexResetsSummary {
  const watch = status.activeWatch;
  if (watch && Date.parse(watch.expiresAt) > now) {
    return { kind: "watch", chancePercent: watch.resetChancePercent };
  }
  const scheduledFor = status.scheduledReset?.scheduledFor;
  if (scheduledFor && Date.parse(scheduledFor) > now) {
    return { kind: "scheduled", resetsAt: scheduledFor };
  }
  return { kind: "noSignal" };
}

export interface CodexResetsDetailLine {
  key: "lastReset" | "scheduled" | "watch";
  at?: string;
  /** "regular" | "banked" */
  resetType?: string;
  chancePercent?: number | null;
  expiresAt?: string;
}

/** 悬浮详情的各行（不含来源声明）；过期的观测不列 */
export function codexResetsDetailLines(
  status: CodexResetsStatus,
  now = Date.now(),
): CodexResetsDetailLine[] {
  const lines: CodexResetsDetailLine[] = [];
  if (status.latestReset) {
    lines.push({
      key: "lastReset",
      at: status.latestReset.announcedAt,
      resetType: status.latestReset.resetType,
    });
  }
  const scheduled = status.scheduledReset;
  if (scheduled?.scheduledFor) {
    lines.push({
      key: "scheduled",
      at: scheduled.scheduledFor,
      resetType: scheduled.resetType,
    });
  }
  const watch = status.activeWatch;
  if (watch && Date.parse(watch.expiresAt) > now) {
    lines.push({
      key: "watch",
      chancePercent: watch.resetChancePercent,
      expiresAt: watch.expiresAt,
    });
  }
  return lines;
}

/** ISO → 「10月25日 01:00」这样的本地绝对时刻 */
export function absoluteTime(iso: string, locale: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime())
    ? iso
    : date.toLocaleString(locale, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
}

function resetTypeLabel(t: TFunction, resetType: string | undefined): string {
  if (resetType === "regular") {
    return t("quota.resetPrediction.typeRegular");
  }
  if (resetType === "banked") {
    return t("quota.resetPrediction.typeBanked");
  }
  return resetType ?? "";
}

/** 详情行 → 文案（「上次重置 · 17 小时前（常规）」） */
export function codexResetsDetailText(
  t: TFunction,
  locale: string,
  line: CodexResetsDetailLine,
  now = Date.now(),
): string {
  if (line.key === "lastReset") {
    const relative = line.at
      ? formatRelativeTime(new Date(line.at).getTime(), now, t)
      : "";
    return t("quota.resetPrediction.lastResetLine", {
      time: relative,
      type: resetTypeLabel(t, line.resetType),
    });
  }
  if (line.key === "scheduled") {
    return t("quota.resetPrediction.scheduledLine", {
      time: line.at ? absoluteTime(line.at, locale) : "",
      type: resetTypeLabel(t, line.resetType),
    });
  }
  if (line.chancePercent != null) {
    return t("quota.resetPrediction.watch", { chance: line.chancePercent });
  }
  return t("quota.resetPrediction.watchActive");
}

/** 一行式短摘要（主窗口 chip / 托盘行共用）：没信号、查询中、失败时只写「重置预测」 */
export function codexResetsChipText(
  t: TFunction,
  status: CodexResetsStatus | undefined,
  now = Date.now(),
): string {
  if (!status) return t("quota.resetPrediction.label");
  const summary = summarizeCodexResets(status, now);
  if (summary.kind === "watch") {
    return summary.chancePercent != null
      ? t("quota.resetPrediction.chipWatch", { chance: summary.chancePercent })
      : t("quota.resetPrediction.label");
  }
  if (summary.kind === "scheduled") {
    const countdown = countdownStr(summary.resetsAt, now);
    return countdown
      ? t("quota.resetPrediction.chipScheduled", { time: countdown })
      : t("quota.resetPrediction.label");
  }
  return t("quota.resetPrediction.label");
}

/**
 * 信号状态（弹窗里紧跟标题的那一行、托盘问号后面的状态）：有观测写概率，
 * 有预告写倒计时，都没有写「当前没有重置信号」，无数据写查询中/暂无数据。
 * 与摘要同一次序（watch > scheduled > noSignal）。
 */
export function codexResetsSignalText(
  t: TFunction,
  status: CodexResetsStatus | undefined,
  isLoading = false,
  now = Date.now(),
): string {
  if (!status) {
    return t(
      isLoading
        ? "quota.resetPrediction.loading"
        : "quota.resetPrediction.unavailable",
    );
  }
  const summary = summarizeCodexResets(status, now);
  if (summary.kind === "watch") {
    return summary.chancePercent != null
      ? t("quota.resetPrediction.watch", { chance: summary.chancePercent })
      : t("quota.resetPrediction.watchActive");
  }
  if (summary.kind === "scheduled") {
    const countdown = countdownStr(summary.resetsAt, now);
    return countdown
      ? t("quota.resetPrediction.chipScheduled", { time: countdown })
      : t("quota.resetPrediction.noSignal");
  }
  return t("quota.resetPrediction.noSignal");
}
