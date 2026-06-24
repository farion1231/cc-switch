import type { UsageData } from "@/types";

interface UsageSummaryLabels {
  invalid: string;
  remaining: string;
  used: string;
}

function formatNumber(value: number): string {
  return Number.isInteger(value) ? value.toString() : value.toFixed(2);
}

/**
 * Unit tokens whose quantity is naturally fractional (currency).
 * Values in these units keep 2 decimal places, matching the prior
 * `.toFixed(2)` behaviour of the usage footer. Everything else
 * (tokens / 次 / points / requests / ... ) renders as an integer,
 * since APIs return whole quantities for those units.
 *
 * Kept as a Set for O(1) lookup; matched case-sensitively because
 * currency codes (USD/CNY/...) are conventionally upper-case and the
 * symbol forms ($/¥/€) have no case variants.
 */
const CURRENCY_UNITS = new Set([
  "USD",
  "CNY",
  "EUR",
  "GBP",
  "JPY",
  "$",
  "¥",
  "€",
  "£",
]);

/**
 * Format a usage quantity for display with smart decimal precision and
 * thousands separators.
 *
 * - Currency units → 2 decimals (e.g. `12.50`).
 * - Non-currency units (tokens / 次 / points / ...) → integer (e.g. `5,000,000`).
 * - No unit → keep the existing integer/2-decimal adaptive behaviour so
 *   call sites that previously relied on it are unchanged.
 *
 * Thousands separators are applied to every numeric value regardless of
 * unit, so large token counts become readable (`12,000,000` instead of
 * `12000000`). See issue #4456.
 *
 * `toLocaleString('en-US', ...)` is used so the separators are `,`
 * (matching the rest of the app's `en-US` numeric formatting) and the
 * output is deterministic across locales.
 */
export function formatUsageValue(value: number, unit?: string): string {
  if (!unit) {
    // Preserve the prior adaptive behaviour: integers stay integers,
    // fractional values keep 2 decimals — but now with thousands
    // separators applied.
    const fractionDigits = Number.isInteger(value) ? 0 : 2;
    return value.toLocaleString("en-US", {
      minimumFractionDigits: fractionDigits,
      maximumFractionDigits: fractionDigits,
    });
  }

  if (unit === "%") {
    return `${formatNumber(value)}%`;
  }

  const decimals = CURRENCY_UNITS.has(unit) ? 2 : 0;
  return value.toLocaleString("en-US", {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
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
): string {
  const planPrefix = data.planName ? `[${data.planName}] ` : "";

  if (data.isValid === false) {
    return `${planPrefix}${data.invalidMessage || labels.invalid}`;
  }

  const parts = [
    formatUsed(data, labels),
    isNumber(data.remaining)
      ? `${labels.remaining} ${formatValue(data.remaining, data.unit)}`
      : null,
    data.extra || null,
  ].filter((part): part is string => Boolean(part));

  return `${planPrefix}${parts.join(" / ") || labels.invalid}`;
}
