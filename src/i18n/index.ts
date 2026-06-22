import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./locales/en.json";
import ja from "./locales/ja.json";
import zh from "./locales/zh.json";
import zhTW from "./locales/zh-TW.json";

type Language = "zh" | "zh-TW" | "en" | "ja";

// English-first build: a fresh install defaults to English on any OS locale.
// Users can still switch languages; the choice is remembered in localStorage.
const DEFAULT_LANGUAGE: Language = "en";

const isSupportedLanguage = (value: string | null): value is Language =>
  value === "zh" || value === "zh-TW" || value === "en" || value === "ja";

const getInitialLanguage = (): Language => {
  if (typeof window !== "undefined") {
    try {
      const stored = window.localStorage.getItem("language");
      if (isSupportedLanguage(stored)) {
        return stored;
      }
    } catch (error) {
      console.warn("[i18n] Failed to read stored language preference", error);
    }
  }

  return DEFAULT_LANGUAGE;
};

const resources = {
  en: {
    translation: en,
  },
  ja: {
    translation: ja,
  },
  zh: {
    translation: zh,
  },
  "zh-TW": {
    translation: zhTW,
  },
};

i18n.use(initReactI18next).init({
  resources,
  lng: getInitialLanguage(), // stored preference, otherwise English
  fallbackLng: "en", // fall back to English when a key is missing

  interpolation: {
    escapeValue: false, // React already escapes values
  },

  // Show debug info in development
  debug: false,
});

export default i18n;
