import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { isTauri } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";

import en from "./locales/en.json";
import ja from "./locales/ja.json";
import zh from "./locales/zh.json";
import zhTW from "./locales/zh-TW.json";

import {
  type Language,
  LANGUAGE_CHANGED_EVENT,
  isSupportedLanguage,
  normalizeLanguage,
} from "./utils";

export {
  type Language,
  LANGUAGE_CHANGED_EVENT,
  DEFAULT_LANGUAGE,
  isSupportedLanguage,
  normalizeLanguage,
} from "./utils";

const getInitialLanguage = (): Language => {
  if (typeof window !== "undefined") {
    try {
      const stored = window.localStorage.getItem("language");
      if (isSupportedLanguage(stored)) {
        return normalizeLanguage(stored);
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

  return normalizeLanguage(navigatorLang);
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
  lng: getInitialLanguage(), // 根据本地存储或系统语言选择默认语言
  fallbackLng: "en", // 如果缺少中文翻译则退回英文

  interpolation: {
    escapeValue: false, // React 已经默认转义
  },

  // 开发模式下显示调试信息
  debug: false,
});

let isSyncingFromEvent = false;
let activeCrossWindowSyncCleanup: (() => void) | null = null;

/**
 * 跨窗口语言同步：当某个窗口（如主窗口）切换语言时，广播事件给其他窗口（如用量悬浮窗）。
 * 其他窗口收到事件后，同步更新其在内存中的 i18n 实例与 localStorage。
 */
export function initI18nCrossWindowSync(): () => void {
  if (typeof window === "undefined" || !isTauri()) {
    return () => {};
  }

  if (activeCrossWindowSyncCleanup) {
    return activeCrossWindowSyncCleanup;
  }

  let lastEmittedLanguage: Language | null = null;
  let unlisten: (() => void) | undefined;
  let disposed = false;

  listen<Language>(LANGUAGE_CHANGED_EVENT, (event) => {
    const incoming = normalizeLanguage(event.payload);

    // 忽略本窗口刚发出的广播
    if (incoming === lastEmittedLanguage) {
      lastEmittedLanguage = null;
      return;
    }

    if (normalizeLanguage(i18n.language) !== incoming) {
      isSyncingFromEvent = true;
      try {
        window.localStorage.setItem("language", incoming);
      } catch (error) {
        console.warn("[i18n] Failed to persist synchronized language", error);
      }
      void i18n.changeLanguage(incoming).finally(() => {
        isSyncingFromEvent = false;
      });
    }
  })
    .then((off) => {
      if (disposed) {
        off();
      } else {
        unlisten = off;
      }
    })
    .catch((error) => {
      console.debug("[i18n] Failed to listen for language changes:", error);
    });

  const onLanguageChanged = (lng: string) => {
    if (isSyncingFromEvent) return;
    const normalized = normalizeLanguage(lng);
    lastEmittedLanguage = normalized;
    try {
      window.localStorage.setItem("language", normalized);
    } catch (error) {
      console.warn("[i18n] Failed to persist language change", error);
    }
    emit(LANGUAGE_CHANGED_EVENT, normalized).catch((error) => {
      console.debug("[i18n] Failed to broadcast language change:", error);
    });
  };

  i18n.on("languageChanged", onLanguageChanged);

  activeCrossWindowSyncCleanup = () => {
    disposed = true;
    unlisten?.();
    i18n.off("languageChanged", onLanguageChanged);
    activeCrossWindowSyncCleanup = null;
  };

  return activeCrossWindowSyncCleanup;
}

export default i18n;
