import i18n, { type BackendModule, type ResourceKey } from "i18next";
import { initReactI18next } from "react-i18next";
import { isLanguage, languageFromLocale, type Language } from "./languages";

const LOCALE_LOADERS: Record<
  Language,
  () => Promise<{ default: ResourceKey }>
> = {
  de: () => import("./locales/de.json"),
  en: () => import("./locales/en.json"),
  ja: () => import("./locales/ja.json"),
  ko: () => import("./locales/ko.json"),
  ru: () => import("./locales/ru.json"),
  tr: () => import("./locales/tr.json"),
  zh: () => import("./locales/zh.json"),
  "zh-TW": () => import("./locales/zh-TW.json"),
};

const inFlightLoads = new Map<Language, Promise<ResourceKey>>();

const loadLocaleResource = (language: Language): Promise<ResourceKey> => {
  const existing = inFlightLoads.get(language);
  if (existing) return existing;
  const promise = LOCALE_LOADERS[language]()
    .then((module) => module.default)
    .finally(() => {
      inFlightLoads.delete(language);
    });
  inFlightLoads.set(language, promise);
  return promise;
};

const hasPopulatedBundle = (language: Language): boolean => {
  if (!i18n.hasResourceBundle(language, "translation")) return false;
  const bundle = i18n.getResourceBundle(language, "translation") as
    | Record<string, unknown>
    | undefined;
  return Boolean(bundle && Object.keys(bundle).length > 0);
};

export const ensureLocaleLoaded = async (language: Language): Promise<void> => {
  if (hasPopulatedBundle(language)) return;
  const resource = await loadLocaleResource(language);
  i18n.addResourceBundle(language, "translation", resource, true, true);
};

const localeBackend: BackendModule = {
  type: "backend",
  init() {},
  read(language, _namespace, callback) {
    if (!isLanguage(language)) {
      callback(null, {});
      return;
    }
    loadLocaleResource(language)
      .then((resource) => callback(null, resource))
      .catch((error) => callback(error as Error, null));
  },
};

const getInitialLanguage = (): Language => {
  if (typeof window !== "undefined") {
    try {
      const stored = window.localStorage.getItem("language");
      if (isLanguage(stored)) {
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

  return languageFromLocale(navigatorLang ?? "");
};

const initialLanguage = getInitialLanguage();

export const i18nReady: Promise<void> = i18n
  .use(localeBackend)
  .use(initReactI18next)
  .init({
    lng: initialLanguage, // 根据本地存储或系统语言按需加载初始语言
    fallbackLng: "en", // 如果缺少翻译则退回英文
    load: "currentOnly",
    partialBundledLanguages: true,

    interpolation: {
      escapeValue: false, // React 已经默认转义
    },

    react: {
      useSuspense: false,
    },

    // 开发模式下显示调试信息
    debug: false,
  })
  .then(async () => {
    await ensureLocaleLoaded(initialLanguage);
    if (initialLanguage !== "en") {
      await ensureLocaleLoaded("en");
    }
  });

/**
 * 用后端 settings.json 里保存的语言校正界面语言。
 *
 * 首屏语言取自 localStorage（同步可读），但它只是缓存：开发版与正式版的
 * WebView 来源不同、WebView 数据被清空时都会读不到或读到旧值，界面就落到
 * 系统语言。启动后拿到后端设置再对一次，不一致时切换并回写缓存。
 */
export const syncLanguageFromSettings = async (
  language?: string | null,
): Promise<void> => {
  if (!isLanguage(language)) return;

  await i18nReady;
  await ensureLocaleLoaded(language);
  if (i18n.language !== language) {
    await i18n.changeLanguage(language);
  }

  try {
    if (window.localStorage.getItem("language") !== language) {
      window.localStorage.setItem("language", language);
    }
  } catch (error) {
    console.warn("[i18n] Failed to persist language preference", error);
  }
};

export default i18n;
