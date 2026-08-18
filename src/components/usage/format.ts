import type {
  AgentUsageMeasure,
  AgentUsageSourceDimension,
} from "@/types/usage";

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

export type UsageCostStatus = "reported" | "estimated" | "unavailable";

export function usageSourceDimensionsForScope(
  sourceDimensions: AgentUsageSourceDimension[] | undefined,
  isDescendant: boolean,
): AgentUsageSourceDimension[] {
  return (
    sourceDimensions?.filter(
      (dimension) => dimension.isDescendant === isDescendant,
    ) ?? []
  );
}

export function resolveUsageCostStatus(
  sourceDimensions: AgentUsageSourceDimension[] | undefined,
): UsageCostStatus {
  if (
    sourceDimensions?.some(
      (dimension) => dimension.costStatus === "unavailable",
    )
  ) {
    return "unavailable";
  }
  if (
    sourceDimensions?.some((dimension) => dimension.costStatus === "estimated")
  ) {
    return "estimated";
  }
  return "reported";
}

/** Resolve the status for one displayed measure, not only its sources. */
export function resolveUsageCostStatusForMeasure(
  measure: AgentUsageMeasure | null | undefined,
  sourceDimensions: AgentUsageSourceDimension[] | undefined,
): UsageCostStatus {
  if (!measure || measure.totalCostUsd == null) return "unavailable";
  return resolveUsageCostStatus(sourceDimensions);
}

export function isCodexReplayInProgress(
  sourceDimensions: AgentUsageSourceDimension[] | undefined,
): boolean {
  return Boolean(
    sourceDimensions?.some(
      (dimension) => dimension.costSource === "codex_replay",
    ),
  );
}

export function formatUsageCost(
  measure: AgentUsageMeasure | null | undefined,
  sourceDimensions: AgentUsageSourceDimension[] | undefined,
  fallback = "—",
): string {
  return formatUsageCostWithStatus(
    measure,
    resolveUsageCostStatusForMeasure(measure, sourceDimensions),
    fallback,
  );
}

export function formatUsageCostWithStatus(
  measure: AgentUsageMeasure | null | undefined,
  status: UsageCostStatus,
  fallback = "—",
): string {
  if (status === "unavailable" || !measure || measure.totalCostUsd == null) {
    return fallback;
  }
  const value = fmtUsd(measure.totalCostUsd, 4, fallback);
  return status === "estimated" ? `≈${value}` : value;
}

/**
 * Display the sum of token components that the source actually provided.
 *
 * A trailing `+` means at least this many tokens are known; a missing
 * component is never silently treated as zero. This is shared by the task
 * table and the selected-session summary so partial Codex facts stay visible
 * in both entry points.
 */
export function formatKnownTokenTotal(
  measure:
    | Pick<
        AgentUsageMeasure,
        | "inputTokens"
        | "outputTokens"
        | "cacheReadTokens"
        | "cacheCreationTokens"
      >
    | null
    | undefined,
  language?: string,
  fallback = "—",
): string {
  if (!measure) return fallback;

  const values = [
    measure.inputTokens,
    measure.outputTokens,
    measure.cacheReadTokens,
    measure.cacheCreationTokens,
  ];
  const known = values.filter((value): value is number => value != null);
  if (known.length === 0) return fallback;

  const total = known.reduce((sum, value) => sum + value, 0);
  return `${fmtInt(total, language, fallback)}${known.length === values.length ? "" : "+"}`;
}

function normalizeLanguageTag(language: string): string {
  return language.toLowerCase().replace(/_/g, "-");
}

function isTraditionalChineseLanguage(normalizedLanguage: string): boolean {
  return (
    normalizedLanguage === "zh-tw" ||
    normalizedLanguage.startsWith("zh-hant") ||
    normalizedLanguage.startsWith("zh-hk") ||
    normalizedLanguage.startsWith("zh-mo")
  );
}

export function getLocaleFromLanguage(language: string): string {
  if (!language) return "en-US";
  const normalized = normalizeLanguageTag(language);
  if (normalized === "zh") return "zh-CN";
  if (isTraditionalChineseLanguage(normalized)) {
    return "zh-TW";
  }
  if (normalized.startsWith("zh")) return "zh-CN";
  if (normalized.startsWith("ja")) return "ja-JP";
  return "en-US";
}

interface I18nLike {
  resolvedLanguage?: string;
  language?: string;
}

export function getResolvedLang(i18n: I18nLike): string {
  return i18n.resolvedLanguage || i18n.language || "en";
}

/**
 * Token 数量的紧凑显示。
 *
 * Why: 中日文用户期待 "亿/万" 量纲；英文用户期待 K/M/B。共用同一份格式化
 * 逻辑避免 Hero 卡和分应用卡显示不一致。`compactDecimals=2` 用于 Hero
 * 大数副标（更精确），默认 1 位用于卡片副字段。
 */
export function formatTokensShort(
  value: number,
  lang: string,
  compactDecimals: 1 | 2 = 1,
): string {
  if (!Number.isFinite(value) || value <= 0) return "0";
  const decimals = compactDecimals;
  const normalizedLang = normalizeLanguageTag(lang);
  if (isTraditionalChineseLanguage(normalizedLang)) {
    if (value >= 1e8) return `${(value / 1e8).toFixed(2)} 億`;
    if (value >= 1e4) return `${(value / 1e4).toFixed(decimals)} 萬`;
    return value.toLocaleString("zh-TW");
  }
  if (normalizedLang.startsWith("zh") || normalizedLang.startsWith("ja")) {
    if (value >= 1e8) return `${(value / 1e8).toFixed(2)} 亿`;
    if (value >= 1e4) return `${(value / 1e4).toFixed(decimals)} 万`;
    return value.toLocaleString();
  }
  if (value >= 1e9) return `${(value / 1e9).toFixed(2)}B`;
  if (value >= 1e6) return `${(value / 1e6).toFixed(2)}M`;
  if (value >= 1e3) return `${(value / 1e3).toFixed(decimals)}K`;
  return value.toLocaleString();
}
