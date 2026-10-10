import type { UsageData } from "@/types";

export type UsageDisplayMode = "remaining" | "used" | "both";

interface UsageSummaryLabels {
  invalid: string;
  remaining: string;
  used: string;
}

function formatNumber(value: number): string {
  return Number.isInteger(value) ? value.toString() : value.toFixed(2);
}

function formatValue(value: number, unit?: string): string {
  if (!unit) {
    return formatNumber(value);
  }

  return unit === "%"
    ? `${formatNumber(value)}%`
    : `${formatNumber(value)} ${unit}`;
}

function isNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function formatUsed(
  data: UsageData,
  labels: UsageSummaryLabels,
): string | null {
  if (!isNumber(data.used)) {
    return null;
  }

  if (isNumber(data.total) && data.total > 0) {
    const usedPercent = (data.used / data.total) * 100;

    if (data.unit === "%" && data.total === 100) {
      return `${labels.used} ${formatValue(data.used, "%")}`;
    }

    return `${labels.used} ${formatNumber(usedPercent)}%`;
  }

  return `${labels.used} ${formatValue(data.used, data.unit)}`;
}

export function formatUsageDataSummary(
  data: UsageData,
  labels: UsageSummaryLabels,
  mode: UsageDisplayMode = "remaining",
): string {
  const planPrefix = data.planName ? `[${data.planName}] ` : "";

  if (data.isValid === false) {
    return `${planPrefix}${data.invalidMessage || labels.invalid}`;
  }

  const showUsed = mode === "used" || mode === "both";
  const showRemaining = mode === "remaining" || mode === "both";

  const remainingPart =
    showRemaining && isNumber(data.remaining)
      ? `${labels.remaining} ${formatValue(data.remaining, data.unit)}`
      : null;

  const usedPart = showUsed ? formatUsed(data, labels) : null;

  // 向后兼容：remaining 模式下如果没有 remaining 数据，回退到显示 used
  const fallbackUsed =
    mode === "remaining" && remainingPart === null ? formatUsed(data, labels) : null;

  const parts = [usedPart || fallbackUsed, remainingPart, data.extra || null].filter(
    (part): part is string => Boolean(part),
  );

  return `${planPrefix}${parts.join(" / ") || labels.invalid}`;
}
