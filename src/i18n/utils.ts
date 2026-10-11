export type Language = "zh" | "zh-TW" | "en" | "ja";

export const LANGUAGE_CHANGED_EVENT = "language-changed";

export const DEFAULT_LANGUAGE: Language = "zh";

export const isSupportedLanguage = (lang?: string | null): lang is Language => {
  if (!lang) return false;
  const normalized = lang.toLowerCase().replace(/_/g, "-");
  return (
    normalized.startsWith("en") ||
    normalized.startsWith("ja") ||
    normalized.startsWith("zh")
  );
};

export const normalizeLanguage = (lang?: string | null): Language => {
  if (!lang) return DEFAULT_LANGUAGE;
  const normalized = lang.toLowerCase().replace(/_/g, "-");

  if (normalized === "zh") {
    return "zh";
  }

  if (
    normalized === "zh-tw" ||
    normalized.startsWith("zh-hant") ||
    normalized.startsWith("zh-hk") ||
    normalized.startsWith("zh-mo")
  ) {
    return "zh-TW";
  }

  if (normalized.startsWith("ja")) {
    return "ja";
  }

  if (normalized.startsWith("en")) {
    return "en";
  }

  if (normalized.startsWith("zh")) {
    return "zh";
  }

  return DEFAULT_LANGUAGE;
};
