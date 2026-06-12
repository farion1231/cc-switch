import i18n from "@/i18n";
import {
  DEFAULT_LANGUAGE,
  normalizeLanguage,
  type Language,
} from "@/i18n/languages";

export function getLocaleFromLanguage(language: string): string {
  if (!language) return "en-US";
  const normalized = language.toLowerCase().replace(/_/g, "-");
  if (
    normalized === "zh-tw" ||
    normalized.startsWith("zh-hant") ||
    normalized.startsWith("zh-hk") ||
    normalized.startsWith("zh-mo")
  ) {
    return "zh-TW";
  }
  if (normalized.startsWith("zh")) return "zh-CN";
  if (normalized.startsWith("ja")) return "ja-JP";
  if (normalized.startsWith("ru")) return "ru-RU";
  return "en-US";
}

export function getActiveLanguage(): Language {
  const language = i18n.resolvedLanguage || i18n.language || DEFAULT_LANGUAGE;
  return normalizeLanguage(language);
}
