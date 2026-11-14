/**
 * 界面语言的唯一来源：设置页下拉的顺序和文案、语言码校验、系统区域识别、
 * 日期数字用的 locale 都从这里取。后端（settings.rs 的 LANGUAGES、
 * tray.rs 的 map_locale_to_tray_language）和这里由
 * tests/config/languages.test.ts 比对 tests/config/languages.json。
 */
export const LANGUAGES = [
  { id: "zh", labelKey: "settings.languageOptionChinese", locale: "zh-CN" },
  {
    id: "zh-TW",
    labelKey: "settings.languageOptionTraditionalChinese",
    locale: "zh-TW",
  },
  { id: "en", labelKey: "settings.languageOptionEnglish", locale: "en-US" },
  { id: "de", labelKey: "settings.languageOptionGerman", locale: "de-DE" },
  { id: "ja", labelKey: "settings.languageOptionJapanese", locale: "ja-JP" },
  { id: "ko", labelKey: "settings.languageOptionKorean", locale: "ko-KR" },
  { id: "ru", labelKey: "settings.languageOptionRussian", locale: "ru-RU" },
  { id: "tr", labelKey: "settings.languageOptionTurkish", locale: "tr-TR" },
] as const;

export type Language = (typeof LANGUAGES)[number]["id"];

export const DEFAULT_LANGUAGE: Language = "zh";

export const isLanguage = (value: unknown): value is Language =>
  LANGUAGES.some((language) => language.id === value);

/** 小写、下划线换成连字符后的语言标签。 */
export const normalizeLanguageTag = (tag: string): string =>
  tag.toLowerCase().replace(/_/g, "-");

/** 繁体中文的区域标签（zh-TW / zh-HK / zh-MO / zh-Hant 及其子标签），参数须已小写。 */
export function isTraditionalChineseTag(tag: string): boolean {
  return ["zh-tw", "zh-hk", "zh-mo", "zh-hant"].some(
    (prefix) => tag === prefix || tag.startsWith(`${prefix}-`),
  );
}

/**
 * 系统区域对应的界面语言：繁中区域用 zh-TW，其余中文用 zh，界面有的语言
 * 按主标签认，认得出但界面没有的语言（法、西等）用英文：英文大多数人能读，
 * 简中几乎不能。区域为空时用默认的 zh。
 */
export function languageFromLocale(locale: string): Language {
  const tag = locale.toLowerCase();
  if (!tag) return DEFAULT_LANGUAGE;
  if (isTraditionalChineseTag(tag)) return "zh-TW";
  const primary = tag.split("-")[0];
  const match = LANGUAGES.find((language) => language.id === primary);
  return match ? match.id : "en";
}
