import {
  isNonNegativeDecimalString,
  type ModelPricingTimeRule,
} from "@/types/usage";

/**
 * 分时段定价的纯函数：格式渲染 + 表单校验。
 * 这里不碰 React、不碰 i18next，对话框和单元测试都直接用它，
 * 保证「规则说的是什么意思」只有一份实现。
 */

/** bit0 = 周一 … bit6 = 周日，与 Rust 侧 weekday_mask 同口径 */
export const WEEKDAY_BITS = [1, 2, 4, 8, 16, 32, 64] as const;

/** 七天全选 */
export const ALL_WEEKDAYS_MASK = 127;
/** 周一到周五 */
export const WEEKDAY_MON_FRI_MASK = 31;
/** 周六 + 周日 */
export const WEEKEND_MASK = 96;

export const MIN_UTC_OFFSET_MINUTES = -1439;
export const MAX_UTC_OFFSET_MINUTES = 1439;
export const MIN_MINUTE_OF_DAY = 0;
export const MAX_START_MINUTE = 1439;
export const MAX_END_MINUTE = 1440;

const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;
const INTEGER_PATTERN = /^-?\d+$/;
const MINUTE_OF_DAY_PATTERN = /^(\d{1,2}):(\d{2})$/;

/** 摘要行里的星期缩写。整行以模型 id、HH:MM、UTC±HH:MM、ISO 日期为主，这些都不本地化 */
export const WEEKDAY_SHORT_NAMES = [
  "Mon",
  "Tue",
  "Wed",
  "Thu",
  "Fri",
  "Sat",
  "Sun",
] as const;

export interface UtcOffsetPreset {
  offset: number;
  /** 只做展示，不参与匹配；固定偏移没有夏令时，实行夏令时的时区要注明按标准时间取 */
  zone: string;
  observesDst: boolean;
}

/** 常用时区：省得用户自己算偏移分钟数。带时区名的一律按标准时间取值。 */
export const UTC_OFFSET_PRESETS: readonly UtcOffsetPreset[] = [
  { offset: 0, zone: "UTC", observesDst: false },
  { offset: 480, zone: "Asia/Shanghai", observesDst: false },
  { offset: 540, zone: "Asia/Tokyo", observesDst: false },
  { offset: 330, zone: "Asia/Kolkata", observesDst: false },
  { offset: 60, zone: "Europe/Berlin", observesDst: true },
  { offset: -300, zone: "America/New_York", observesDst: true },
  { offset: -480, zone: "America/Los_Angeles", observesDst: true },
];

/** `UTC+08:00` / `UTC-05:30` / `UTC+00:00` */
export function formatUtcOffset(minutes: number): string {
  if (!Number.isInteger(minutes)) return "UTC?";
  const sign = minutes < 0 ? "-" : "+";
  const absolute = Math.abs(minutes);
  const hours = String(Math.floor(absolute / 60)).padStart(2, "0");
  const rest = String(absolute % 60).padStart(2, "0");
  return `UTC${sign}${hours}:${rest}`;
}

/** 解析固定偏移输入（分钟）。空串 / 非整数 / 小数一律算没填对。 */
export function parseUtcOffset(text: string): number | null {
  const trimmed = text.trim();
  if (!INTEGER_PATTERN.test(trimmed)) return null;
  const value = Number(trimmed);
  return Number.isInteger(value) ? value : null;
}

/** 一天内的第几分钟 → `HH:MM`；1440 渲染成 `24:00`（窗口右端点的合法值） */
export function formatMinuteOfDay(minute: number): string {
  if (!Number.isInteger(minute) || minute < 0) return "--:--";
  const hours = String(Math.floor(minute / 60)).padStart(2, "0");
  const rest = String(minute % 60).padStart(2, "0");
  return `${hours}:${rest}`;
}

/** `HH:MM` → 分钟；`24:00` → 1440。认不出一律 null。 */
export function parseMinuteOfDay(text: string): number | null {
  const match = MINUTE_OF_DAY_PATTERN.exec(text.trim());
  if (!match) return null;
  const minutes = Number(match[2]);
  if (minutes > 59) return null;
  const total = Number(match[1]) * 60 + minutes;
  if (total > MAX_END_MINUTE) return null;
  return total;
}

/**
 * 星期掩码 → `Mon–Fri` / `Sat–Sun` / `Mon, Wed` / `Every day`。
 *
 * 连续的日期压成区间，按周一…周日的顺序输出——和编辑器里那排复选框同一个顺序，
 * 用户对着念就能确认。跨周的连续段（周五到周一）压不成一段，会写成
 * `Mon, Fri–Sun`：宁可少压一段，也不要把顺序打乱或者漏掉哪天。
 */
export function formatWeekdayMask(mask: number): string {
  if (!Number.isInteger(mask) || mask <= 0) return "--";
  const active = WEEKDAY_BITS.map((bit, index) =>
    mask & bit ? index : -1,
  ).filter((index) => index >= 0);
  // 位全落在 bit0..bit6 之外（存储被手改坏的情况）：不猜，直接说看不出来
  if (active.length === 0) return "--";
  if (active.length === WEEKDAY_BITS.length) return "Every day";

  // active 由 WEEKDAY_BITS 顺序产生，本身就是升序，线性扫一遍就能压区间
  const runs: number[][] = [];
  for (const index of active) {
    const last = runs[runs.length - 1];
    if (last && index === last[last.length - 1] + 1) last.push(index);
    else runs.push([index]);
  }

  return runs
    .map((run) =>
      run.length === 1
        ? WEEKDAY_SHORT_NAMES[run[0]]
        : `${WEEKDAY_SHORT_NAMES[run[0]]}–${WEEKDAY_SHORT_NAMES[run[run.length - 1]]}`,
    )
    .join(", ");
}

/** `2026-10-01,2026-10-02` / 空格分隔都认；空串 → 空数组 */
export function parseExcludedDates(text: string): string[] {
  return text
    .split(/[\s,]+/)
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
}

/** 格式对还不够：`2026-02-30` 这种不存在的日期要拦住 */
export function isRealCalendarDate(value: string): boolean {
  if (!DATE_PATTERN.test(value)) return false;
  const [year, month, day] = value.split("-").map(Number);
  const date = new Date(Date.UTC(year, month - 1, day));
  return (
    date.getUTCFullYear() === year &&
    date.getUTCMonth() === month - 1 &&
    date.getUTCDate() === day
  );
}

/**
 * 一条规则念起来是什么样，例如：
 * `Peak hours · deepseek-chat · Mon–Fri 01:00–04:00 (UTC+00:00) · except 2026-10-01`
 *
 * 这是本功能最要紧的一处文案：用户得能一眼确认「规则说的就是我想要的」。
 * 行里除了三个连接词（all day / Every day / except）都是不本地化的内容——
 * 模型 id、HH:MM、UTC±HH:MM、ISO 日期——所以整行固定英文，测试可以直接钉死。
 */
export function describeTimeRule(rule: ModelPricingTimeRule): string {
  const parts: string[] = [];
  const label = rule.label.trim();
  if (label) parts.push(label);
  parts.push(rule.modelId.trim() || "--");

  const weekdays = formatWeekdayMask(rule.weekdayMask);
  const window =
    rule.startMinute === rule.endMinute
      ? "all day"
      : `${formatMinuteOfDay(rule.startMinute)}–${formatMinuteOfDay(rule.endMinute)}`;
  parts.push(
    `${weekdays} ${window} (${formatUtcOffset(rule.utcOffsetMinutes)})`,
  );

  const dates = (rule.excludedDates ?? []).filter((date) => date.trim());
  if (dates.length > 0) {
    parts.push(`except ${dates.join(", ")}`);
  }

  return parts.join(" · ");
}

export type TimeRuleField =
  | "modelId"
  | "utcOffsetMinutes"
  | "weekdayMask"
  | "startMinute"
  | "endMinute"
  | "excludedDates"
  | "inputCostPerMillion"
  | "outputCostPerMillion"
  | "cacheReadCostPerMillion"
  | "cacheCreationCostPerMillion";

export type TimeRuleErrorCode =
  | "modelIdRequired"
  | "utcOffsetNotInteger"
  | "utcOffsetOutOfRange"
  | "weekdayMaskEmpty"
  | "startMinuteOutOfRange"
  | "endMinuteOutOfRange"
  | "excludedDateFormat"
  | "excludedDateNotReal"
  | "priceRequired"
  | "priceInvalid";

export interface TimeRuleFieldError {
  field: TimeRuleField;
  code: TimeRuleErrorCode;
}

/** 编辑器里的草稿：数字字段先按文本存，校验就是校验用户敲进去的那串字符 */
export interface TimeRuleDraft {
  modelId: string;
  label: string;
  utcOffsetText: string;
  weekdayMask: number;
  startText: string;
  endText: string;
  excludedDatesText: string;
  inputCostPerMillion: string;
  outputCostPerMillion: string;
  cacheReadCostPerMillion: string;
  cacheCreationCostPerMillion: string;
}

const PRICE_FIELDS = [
  "inputCostPerMillion",
  "outputCostPerMillion",
  "cacheReadCostPerMillion",
  "cacheCreationCostPerMillion",
] as const satisfies readonly TimeRuleField[];

/**
 * 客户端校验，和 Rust 侧同一套边界。返回错误码列表（文案由界面翻译），
 * 空数组即通过。日期按条报，坏掉第几条就指出第几条。
 */
export function validateTimeRuleDraft(
  draft: TimeRuleDraft,
): TimeRuleFieldError[] {
  const errors: TimeRuleFieldError[] = [];

  if (!draft.modelId.trim()) {
    errors.push({ field: "modelId", code: "modelIdRequired" });
  }

  const offset = parseUtcOffset(draft.utcOffsetText);
  if (offset == null) {
    errors.push({ field: "utcOffsetMinutes", code: "utcOffsetNotInteger" });
  } else if (
    offset < MIN_UTC_OFFSET_MINUTES ||
    offset > MAX_UTC_OFFSET_MINUTES
  ) {
    errors.push({ field: "utcOffsetMinutes", code: "utcOffsetOutOfRange" });
  }

  if (
    !Number.isInteger(draft.weekdayMask) ||
    draft.weekdayMask <= 0 ||
    draft.weekdayMask > ALL_WEEKDAYS_MASK
  ) {
    errors.push({ field: "weekdayMask", code: "weekdayMaskEmpty" });
  }

  const start = parseMinuteOfDay(draft.startText);
  if (start == null || start < MIN_MINUTE_OF_DAY || start > MAX_START_MINUTE) {
    errors.push({ field: "startMinute", code: "startMinuteOutOfRange" });
  }

  const end = parseMinuteOfDay(draft.endText);
  if (end == null || end < MIN_MINUTE_OF_DAY || end > MAX_END_MINUTE) {
    errors.push({ field: "endMinute", code: "endMinuteOutOfRange" });
  }

  for (const date of parseExcludedDates(draft.excludedDatesText)) {
    if (!DATE_PATTERN.test(date)) {
      errors.push({ field: "excludedDates", code: "excludedDateFormat" });
    } else if (!isRealCalendarDate(date)) {
      errors.push({ field: "excludedDates", code: "excludedDateNotReal" });
    }
  }

  for (const field of PRICE_FIELDS) {
    // 空串和 0 都要分开报：空着不填会静默少算钱，跟「这一档就是免费」不是一回事
    if (!draft[field].trim()) {
      errors.push({ field, code: "priceRequired" });
    } else if (!isNonNegativeDecimalString(draft[field])) {
      errors.push({ field, code: "priceInvalid" });
    }
  }

  return errors;
}

/** 校验通过后才能建出规则；认不出的字段返回 null，界面上不出摘要行 */
export function draftToTimeRule(
  draft: TimeRuleDraft,
  id: string,
): ModelPricingTimeRule | null {
  const utcOffsetMinutes = parseUtcOffset(draft.utcOffsetText);
  const startMinute = parseMinuteOfDay(draft.startText);
  const endMinute = parseMinuteOfDay(draft.endText);
  if (utcOffsetMinutes == null || startMinute == null || endMinute == null) {
    return null;
  }

  return {
    id,
    modelId: draft.modelId.trim(),
    label: draft.label.trim(),
    utcOffsetMinutes,
    weekdayMask: draft.weekdayMask,
    startMinute,
    endMinute,
    excludedDates: parseExcludedDates(draft.excludedDatesText),
    inputCostPerMillion: draft.inputCostPerMillion.trim(),
    outputCostPerMillion: draft.outputCostPerMillion.trim(),
    cacheReadCostPerMillion: draft.cacheReadCostPerMillion.trim(),
    cacheCreationCostPerMillion: draft.cacheCreationCostPerMillion.trim(),
  };
}

export function timeRuleToDraft(rule: ModelPricingTimeRule): TimeRuleDraft {
  return {
    modelId: rule.modelId,
    label: rule.label,
    utcOffsetText: String(rule.utcOffsetMinutes),
    weekdayMask: rule.weekdayMask,
    startText: formatMinuteOfDay(rule.startMinute),
    endText: formatMinuteOfDay(rule.endMinute),
    excludedDatesText: (rule.excludedDates ?? []).join(", "),
    inputCostPerMillion: rule.inputCostPerMillion,
    outputCostPerMillion: rule.outputCostPerMillion,
    cacheReadCostPerMillion: rule.cacheReadCostPerMillion,
    cacheCreationCostPerMillion: rule.cacheCreationCostPerMillion,
  };
}

/** 新建规则的初始草稿：全天、每天、UTC。四个单价一律留空——不预填 0，
 *  免得「没来得及填」和「这一档真的免费」长得一样，静默少算钱。 */
export function emptyTimeRuleDraft(): TimeRuleDraft {
  return {
    modelId: "",
    label: "",
    utcOffsetText: "0",
    weekdayMask: ALL_WEEKDAYS_MASK,
    startText: "00:00",
    endText: "00:00",
    excludedDatesText: "",
    inputCostPerMillion: "",
    outputCostPerMillion: "",
    cacheReadCostPerMillion: "",
    cacheCreationCostPerMillion: "",
  };
}

/** 后端发的 id 只在保存后才有；本地新建的先顶一个，保存时整体交给后端重排 */
export function newTimeRuleId(): string {
  const cryptoRef = globalThis.crypto;
  if (typeof cryptoRef?.randomUUID === "function") {
    return cryptoRef.randomUUID();
  }
  return `draft-${Math.random().toString(36).slice(2)}-${Date.now().toString(36)}`;
}
