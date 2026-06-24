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
 * Matched case-sensitively for the symbol forms ($/¥/€/£, which have no
 * case variants) and case-insensitively for the ISO code forms: user
 * extractor scripts may write `"usd"` as easily as `"USD"`, and the
 * `unit` field on `UsageData` is a free-form string.
 */
const CURRENCY_CODES = new Set(["USD", "CNY", "EUR", "GBP", "JPY"]);
const CURRENCY_SYMBOLS = new Set(["$", "¥", "€", "£"]);

function isCurrencyUnit(unit: string): boolean {
  return CURRENCY_SYMBOLS.has(unit) || CURRENCY_CODES.has(unit.toUpperCase());
}

/**
 * Format a usage quantity for display with smart decimal precision and
 * thousands separators.
 *
 * - Currency units → 2 decimals (e.g. `12.50`).
 * - Non-currency units (tokens / 次 / points / ...) → integer (e.g. `5,000,000`).
 * - `%` → adaptive integer/2-decimal (e.g. `45%`, `45.12%`), grouped.
 * - No unit → adaptive integer/2-decimal, grouped.
 *
 * Thousands separators are applied to every numeric value regardless of
 * unit, so large token counts become readable (`12,000,000` instead of
 * `12000000`). See issue #4456.
 *
 * `toLocaleString('en-US', ...)` is used for both grouping and rounding
 * so the separators are `,` and the last-digit rounding is consistent
 * across every branch (note: `toLocaleString` rounds half up, which
 * differs from `toFixed`'s binary-float rounding — using it uniformly
 * avoids a per-unit off-by-one-cent divergence).
 *
 * Non-finite values (`NaN`, `Infinity`) render as `"—"` rather than the
 * literal `"NaN"`/`"∞"` strings, mirroring the `isNumber` guard already
 * used by `formatUsageDataSummary` and avoiding a collision with the
 * `total === -1 → "∞"` sentinel in `UsageFooter`.
 */
export function formatUsageValue(value: number, unit?: string): string {
  if (!Number.isFinite(value)) {
    return "—";
  }

  const fractionDigits = Number.isInteger(value) ? 0 : 2;

  if (!unit) {
    // Preserve the prior adaptive behaviour: integers stay integers,
    // fractional values keep 2 decimals — but now with thousands
    // separators applied.
    return value.toLocaleString("en-US", {
      minimumFractionDigits: fractionDigits,
      maximumFractionDigits: fractionDigits,
    });
  }

  if (unit === "%") {
    // Adaptive like the no-unit branch, so a large percentage stays
    // grouped and the rounding mode matches every other branch.
    return `${value.toLocaleString("en-US", {
      minimumFractionDigits: fractionDigits,
      maximumFractionDigits: fractionDigits,
    })}%`;
  }

  const decimals = isCurrencyUnit(unit) ? 2 : 0;
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
