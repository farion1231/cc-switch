import i18n from "@/i18n";

export function getLocaleFromLanguage(language: string): string {
  if (!language) return "en-US";
  if (language.startsWith("zh")) return "zh-CN";
  if (language.startsWith("ja")) return "ja-JP";
  if (language.startsWith("ru")) return "ru-RU";
  return "en-US";
}

export function getActiveLanguage(): "zh" | "en" | "ja" | "ru" {
  const language = (i18n.resolvedLanguage || i18n.language || "en").toLowerCase();
  if (language.startsWith("zh")) return "zh";
  if (language.startsWith("ja")) return "ja";
  if (language.startsWith("ru")) return "ru";
  return "en";
}
