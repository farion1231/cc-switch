import { useEffect, useState } from "react";

/** 和主界面 ThemeProvider 的 storageKey 一致 */
const THEME_STORAGE_KEY = "cc-switch-theme";

type ThemeChoice = "light" | "dark" | "system";

function readChoice(): ThemeChoice {
  try {
    const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (stored === "light" || stored === "dark") return stored;
  } catch {
    // 读不到按跟随系统
  }
  return "system";
}

function systemDark(): boolean {
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
}

export interface PanelTheme {
  dark: boolean;
  /** 用户选定的深浅色；跟随系统时为 null（窗口外观交回系统） */
  explicit: boolean | null;
}

function resolve(): PanelTheme {
  const choice = readChoice();
  if (choice === "system") return { dark: systemDark(), explicit: null };
  return { dark: choice === "dark", explicit: choice === "dark" };
}

/**
 * 面板是常驻的独立 WebView：不用主界面的 ThemeProvider（它会改主窗口的原生主题），
 * 只读主界面存在 localStorage 的选择，另一个窗口改了或系统深浅色变了都跟着变。
 * `refreshKey` 变化（每次弹出）时也重新读一次。
 */
export function usePanelTheme(refreshKey: unknown): PanelTheme {
  const [theme, setTheme] = useState<PanelTheme>(resolve);

  useEffect(() => {
    setTheme(resolve());
  }, [refreshKey]);

  useEffect(() => {
    const update = () => setTheme(resolve());
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    media?.addEventListener("change", update);
    const onStorage = (event: StorageEvent) => {
      if (event.key === THEME_STORAGE_KEY) update();
    };
    window.addEventListener("storage", onStorage);
    return () => {
      media?.removeEventListener("change", update);
      window.removeEventListener("storage", onStorage);
    };
  }, []);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme.dark);
    document.documentElement.classList.toggle("light", !theme.dark);
  }, [theme.dark]);

  return theme;
}
