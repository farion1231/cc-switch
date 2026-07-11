export const SUPPORTED_LANGUAGES = ["zh", "zh-TW", "en", "ja", "ru"] as const;

export type Language = (typeof SUPPORTED_LANGUAGES)[number];

export const DEFAULT_LANGUAGE: Language = "zh";

export function isSupportedLanguage(value: string): value is Language {
  return (SUPPORTED_LANGUAGES as readonly string[]).includes(value);
}

export function normalizeLanguage(value?: string | null): Language {
  if (!value) return DEFAULT_LANGUAGE;

  const normalized = value.toLowerCase().replace(/_/g, "-");
  if (normalized === "zh") return "zh";
  if (
    normalized === "zh-tw" ||
    normalized.startsWith("zh-hant") ||
    normalized.startsWith("zh-hk") ||
    normalized.startsWith("zh-mo")
  ) {
    return "zh-TW";
  }
  if (normalized.startsWith("en")) return "en";
  if (normalized.startsWith("ja")) return "ja";
  if (normalized.startsWith("ru")) return "ru";
  if (normalized.startsWith("zh")) return "zh";

  return DEFAULT_LANGUAGE;
}
