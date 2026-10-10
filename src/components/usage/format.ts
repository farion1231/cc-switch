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

/** 输出少于这个数的请求（工具调用这类）不算速度：0.1 秒回 15 个 token 会算出离谱的数。 */
export const SPEED_MIN_OUTPUT_TOKENS = 100;

interface OutputTokensPerSecondInput {
  outputTokens: unknown;
  latencyMs: unknown;
  firstTokenMs?: unknown;
  dataSource?: unknown;
}

/**
 * 一条请求的生成时间（毫秒）= 耗时 − 首字。缺耗时或首字、或差值不大于 0 时返回 null。
 * 会话日志没有首字计时，非流式请求也没有，这两类都不算精确速度
 * （会话日志另有估算，见 [getEstimatedTokensPerSecond]）。
 */
export function getGenerationMs(
  log: OutputTokensPerSecondInput,
): number | null {
  const latencyMs = parseFiniteNumber(log.latencyMs);
  const firstTokenMs = parseFiniteNumber(log.firstTokenMs);
  if (latencyMs == null || firstTokenMs == null) return null;
  const generationMs = latencyMs - firstTokenMs;
  return generationMs > 0 ? generationMs : null;
}

// 生成窗口短于此值时不算速度：中转站缓冲后一次性吐出、或短回复整段落在
// 同一个网络包里，窗口只剩几毫秒，算出来的是传输突发而不是生成速度。
export const SPEED_MIN_GENERATION_MS = 100;

/** 这条请求能不能算速度：输出不少于 100 token，且生成时间不短于 100 毫秒。 */
export function isSpeedEligible(log: OutputTokensPerSecondInput): boolean {
  const outputTokens = parseFiniteNumber(log.outputTokens);
  const generationMs = getGenerationMs(log);
  return (
    outputTokens != null &&
    outputTokens >= SPEED_MIN_OUTPUT_TOKENS &&
    generationMs != null &&
    generationMs >= SPEED_MIN_GENERATION_MS
  );
}

/** 单条速度（tok/s）= 输出 token ÷ ((耗时 − 首字) / 1000)；不满足条件返回 null。 */
export function getOutputTokensPerSecond(
  log: OutputTokensPerSecondInput,
): number | null {
  if (!isSpeedEligible(log)) return null;
  const outputTokens = parseFiniteNumber(log.outputTokens) as number;
  const generationMs = getGenerationMs(log) as number;
  const tps = outputTokens / (generationMs / 1000);
  return Number.isFinite(tps) && tps > 0 ? tps : null;
}

/**
 * 估算速度的输出门槛。会话日志导入的请求没有首字计时，耗时是按日志时间戳估的、
 * 含首字等待；输出越少首字占比越大、算出来越偏低，所以门槛比精确口径高。
 */
export const SPEED_ESTIMATE_MIN_OUTPUT_TOKENS = 200;

/** 估算耗时短于此值时不估速度：输出 200 token 以上却不到 1 秒，多半是起点取晚了。 */
export const SPEED_ESTIMATE_MIN_DURATION_MS = 1000;

/** 这条请求是不是从会话日志导入的（不是路由服务记的）。 */
export function isSessionLogRequest(log: { dataSource?: unknown }): boolean {
  return (
    typeof log.dataSource === "string" &&
    log.dataSource !== "" &&
    log.dataSource !== "proxy"
  );
}

/**
 * 这条请求能不能估速度（和后端 `speed_estimate_eligible_sql` 同口径）：会话日志导入、
 * 没有首字计时、输出不少于 200 token、估算耗时不短于 1 秒。
 */
export function isSpeedEstimateEligible(
  log: OutputTokensPerSecondInput,
): boolean {
  const outputTokens = parseFiniteNumber(log.outputTokens);
  const latencyMs = parseFiniteNumber(log.latencyMs);
  return (
    isSessionLogRequest(log) &&
    parseFiniteNumber(log.firstTokenMs) == null &&
    outputTokens != null &&
    outputTokens >= SPEED_ESTIMATE_MIN_OUTPUT_TOKENS &&
    latencyMs != null &&
    latencyMs >= SPEED_ESTIMATE_MIN_DURATION_MS
  );
}

/** 单条估算速度（tok/s）= 输出 token ÷ (估算耗时 / 1000)，含首字等待；不满足条件返回 null。 */
export function getEstimatedTokensPerSecond(
  log: OutputTokensPerSecondInput,
): number | null {
  if (!isSpeedEstimateEligible(log)) return null;
  const outputTokens = parseFiniteNumber(log.outputTokens) as number;
  const latencyMs = parseFiniteNumber(log.latencyMs) as number;
  const tps = outputTokens / (latencyMs / 1000);
  return Number.isFinite(tps) && tps > 0 ? tps : null;
}

/**
 * 汇总速度 = Σ输出 ÷ Σ生成时间（只算满足条件的请求），不是逐条平均——
 * 逐条平均会被短请求带歪。分子分母都来自后端汇总（或 [sumSpeedTotals]）。
 */
export function getAggregateTokensPerSecond(
  outputTokens: unknown,
  generationMs: unknown,
): number | null {
  const output = parseFiniteNumber(outputTokens);
  const ms = parseFiniteNumber(generationMs);
  if (output == null || ms == null || output <= 0 || ms <= 0) return null;
  const tps = output / (ms / 1000);
  return Number.isFinite(tps) && tps > 0 ? tps : null;
}

/** 前端已有明细时的汇总：只累加满足条件的请求。 */
export function sumSpeedTotals(logs: readonly OutputTokensPerSecondInput[]): {
  outputTokens: number;
  generationMs: number;
} {
  let outputTokens = 0;
  let generationMs = 0;
  for (const log of logs) {
    if (!isSpeedEligible(log)) continue;
    outputTokens += parseFiniteNumber(log.outputTokens) as number;
    generationMs += getGenerationMs(log) as number;
  }
  return { outputTokens, generationMs };
}

/** tok/s 的显示：≥ 1 取整，否则一位小数；null 原样返回（界面显示「—」）。 */
export function formatTokensPerSecond(tps: number | null): string | null {
  if (tps == null) return null;
  return tps >= 1 ? Math.round(tps).toString() : tps.toFixed(1);
}

export function formatOutputTokensPerSecond(
  log: OutputTokensPerSecondInput,
): string | null {
  return formatTokensPerSecond(getOutputTokensPerSecond(log));
}

export function formatEstimatedTokensPerSecond(
  log: OutputTokensPerSecondInput,
): string | null {
  return formatTokensPerSecond(getEstimatedTokensPerSecond(log));
}

/**
 * 表格和指标卡里的 Token 数：1 万以下写全（带千分位），以上压成最多 3 位有效数字
 * （48.2K、189K、18.2M），精确值放进 title。四种语言都用 K/M/B，和画板一致。
 */
export function formatTokensCompact(value: unknown, locale?: string): string {
  const num = parseFiniteNumber(value);
  if (num == null) return "--";
  const abs = Math.abs(num);
  if (abs < 10_000) {
    return new Intl.NumberFormat(locale).format(Math.round(num));
  }
  const units: Array<[number, string]> = [
    [1e9, "B"],
    [1e6, "M"],
    [1e3, "K"],
  ];
  for (let i = 0; i < units.length; i++) {
    const [size, suffix] = units[i];
    if (abs >= size) {
      const scaled = Number((num / size).toPrecision(3));
      // 999.95K 进位成 1000K 时改用上一级单位
      if (Math.abs(scaled) >= 1000 && i > 0) {
        const [biggerSize, biggerSuffix] = units[i - 1];
        return `${Number((num / biggerSize).toPrecision(3))}${biggerSuffix}`;
      }
      return `${scaled}${suffix}`;
    }
  }
  return String(num);
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

function isCjkLanguage(lang: string): boolean {
  const normalized = normalizeLanguageTag(lang);
  return normalized.startsWith("zh") || normalized.startsWith("ja");
}

/** 并列的名字：中日文用「、」，其他语言用逗号。 */
export function joinNames(items: readonly string[], lang: string): string {
  return items.join(isCjkLanguage(lang) ? "、" : ", ");
}

/** 并列的分句：中日文用「；」，其他语言用分号。 */
export function joinClauses(items: readonly string[], lang: string): string {
  return items.join(isCjkLanguage(lang) ? "；" : "; ");
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

/** 「刚刚」「N 分钟前」「N 小时前」「N 天前」 */
export function formatRelativeTime(
  timestampMs: number,
  t: (key: string, options?: { count?: number }) => string,
  nowMs: number = Date.now(),
): string {
  const diffMinutes = Math.max(0, Math.floor((nowMs - timestampMs) / 60_000));
  if (diffMinutes < 1) return t("usage.justNow");
  if (diffMinutes < 60) return t("usage.minutesAgo", { count: diffMinutes });
  const hours = Math.floor(diffMinutes / 60);
  if (hours < 24) return t("usage.hoursAgo", { count: hours });
  return t("usage.daysAgo", { count: Math.floor(hours / 24) });
}
