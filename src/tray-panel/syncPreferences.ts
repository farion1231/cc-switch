/**
 * 托盘面板只隐藏不销毁，是独立的 WebView：主界面改了主题 / 语言后，这里还停在建窗口时读的
 * 那个。弹出前和别的窗口改了这两项时，按主界面存在 localStorage 里的值重新套一遍。
 */

/** 和主界面 ThemeProvider 的 storageKey 一致 */
export const THEME_STORAGE_KEY = "cc-switch-theme";
/** 和 useSettings 保存语言用的键一致 */
export const LANGUAGE_STORAGE_KEY = "language";

const THEMES = ["light", "dark", "system"] as const;
const LANGUAGES = ["zh", "zh-TW", "en", "ja"] as const;

type Theme = (typeof THEMES)[number];

export interface PreferenceTargets {
  setTheme: (theme: Theme) => void;
  currentLanguage: string;
  changeLanguage: (language: string) => unknown;
}

export function syncPreferencesFromStorage(
  storage: Pick<Storage, "getItem">,
  targets: PreferenceTargets,
): void {
  const theme = storage.getItem(THEME_STORAGE_KEY);
  if ((THEMES as readonly string[]).includes(theme ?? "")) {
    targets.setTheme(theme as Theme);
  }
  const language = storage.getItem(LANGUAGE_STORAGE_KEY);
  if (
    language &&
    (LANGUAGES as readonly string[]).includes(language) &&
    language !== targets.currentLanguage
  ) {
    void targets.changeLanguage(language);
  }
}

/** storage 事件里只有这两个键要管 */
export function isPreferenceKey(key: string | null): boolean {
  return key === THEME_STORAGE_KEY || key === LANGUAGE_STORAGE_KEY;
}
