import i18n from "@/i18n";
import {
  DEFAULT_LANGUAGE,
  SUPPORTED_LANGUAGES,
  type Language,
} from "@/i18n/languages";

export function getLocaleFromLanguage(language: string): string {
  if (!language) return "en-US";
  if (language.startsWith("zh")) return "zh-CN";
  if (language.startsWith("ja")) return "ja-JP";
  if (language.startsWith("ru")) return "ru-RU";
  return "en-US";
}

export function getActiveLanguage(): Language {
  const language = (
    i18n.resolvedLanguage ||
    i18n.language ||
    DEFAULT_LANGUAGE
  ).toLowerCase();
  const activeLanguage = SUPPORTED_LANGUAGES.find((supportedLanguage) =>
    language.startsWith(supportedLanguage),
  );
  return activeLanguage ?? DEFAULT_LANGUAGE;
}
