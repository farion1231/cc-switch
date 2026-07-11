import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./locales/en.json";
import ja from "./locales/ja.json";
import zh from "./locales/zh.json";
import zhTW from "./locales/zh-TW.json";

type Language = "zh" | "zh-TW" | "en" | "ja";

const DEFAULT_LANGUAGE: Language = "en";

const getInitialLanguage = (): Language => {
  if (typeof window !== "undefined") {
    try {
      const stored = window.localStorage.getItem("language");
      if (
        stored === "zh" ||
        stored === "zh-TW" ||
        stored === "en" ||
        stored === "ja"
      ) {
        return stored;
      }
    } catch (error) {
      console.warn("[i18n] Failed to read stored language preference", error);
    }
  }

  const navigatorLang =
    typeof navigator !== "undefined"
      ? (navigator.language?.toLowerCase() ??
        navigator.languages?.[0]?.toLowerCase())
      : undefined;

  if (navigatorLang === "zh") {
    return "zh";
  }

  if (
    navigatorLang?.startsWith("zh-tw") ||
    navigatorLang?.startsWith("zh-hk") ||
    navigatorLang?.startsWith("zh-mo") ||
    navigatorLang?.startsWith("zh-hant")
  ) {
    return "zh-TW";
  }

  if (navigatorLang?.startsWith("zh")) {
    return "zh";
  }

  if (navigatorLang?.startsWith("ja")) {
    return "ja";
  }

  if (navigatorLang?.startsWith("en")) {
    return "en";
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

// HTML lang 属性用的 BCP 47 标签，供屏幕阅读器选择正确的发音规则
const HTML_LANG_MAP: Record<Language, string> = {
  zh: "zh-CN",
  "zh-TW": "zh-TW",
  en: "en",
  ja: "ja",
};

const applyHtmlLang = (language: string) => {
  if (typeof document === "undefined") return;
  document.documentElement.lang =
    HTML_LANG_MAP[language as Language] ?? language;
};

i18n.use(initReactI18next).init({
  resources,
  lng: getInitialLanguage(), // 根据本地存储或系统语言选择默认语言
  fallbackLng: "en", // 如果缺少中文翻译则退回英文

  interpolation: {
    escapeValue: false, // React 已经默认转义
  },

  // 开发模式下显示调试信息
  debug: false,
});

applyHtmlLang(i18n.language);
i18n.on("languageChanged", applyHtmlLang);

export default i18n;
