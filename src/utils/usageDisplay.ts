import type { UsageData } from "@/types";

export type UsageDisplayOrder = "remaining-first" | "used-first";

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

function formatRemaining(
  data: UsageData,
  labels: UsageSummaryLabels,
): string | null {
  if (!isNumber(data.remaining)) {
    return null;
  }

  return `${labels.remaining} ${formatValue(data.remaining, data.unit)}`;
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
  displayOrder?: UsageDisplayOrder,
): string {
  const planPrefix = data.planName ? `[${data.planName}] ` : "";

  if (data.isValid === false) {
    return `${planPrefix}${data.invalidMessage || labels.invalid}`;
  }

  const order = displayOrder ?? "remaining-first";
  const parts = [
    ...(order === "remaining-first"
      ? [formatRemaining(data, labels), formatUsed(data, labels)]
      : [formatUsed(data, labels), formatRemaining(data, labels)]),
    data.extra || null,
  ].filter((part): part is string => Boolean(part));

  return `${planPrefix}${parts.join(" / ") || labels.invalid}`;
}
