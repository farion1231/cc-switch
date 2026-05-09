export function parseFiniteNumber(value: unknown): number | null {
  if (typeof value === "number") {
    return Number.isFinite(value) ? value : null;
  }

  if (typeof value === "string") {
    const parsed = Number.parseFloat(value);
    return Number.isFinite(parsed) ? parsed : null;
  }

  return null;
}

export function fmtInt(
  value: unknown,
  locale?: string,
  fallback: string = "--",
): string {
  const num = parseFiniteNumber(value);
  if (num == null) return fallback;
  return new Intl.NumberFormat(locale).format(Math.trunc(num));
}

export function fmtUsd(
  value: unknown,
  digits: number,
  fallback: string = "--",
): string {
  const num = parseFiniteNumber(value);
  if (num == null) return fallback;
  return `$${num.toFixed(digits)}`;
}

export function fmtTokenAbbr(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)}B`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}K`;
  return n.toString();
}

export function getLocaleFromLanguage(language: string): string {
  if (!language) return "en-US";
  if (language.startsWith("zh")) return "zh-CN";
  if (language.startsWith("ja")) return "ja-JP";
  return "en-US";
}
