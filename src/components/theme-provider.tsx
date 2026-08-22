import React, {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";

type Theme = "light" | "dark" | "system";

interface ThemeProviderProps {
  children: React.ReactNode;
  defaultTheme?: Theme;
  storageKey?: string;
}

interface ThemeContextValue {
  theme: Theme;
  setTheme: (theme: Theme) => void;
}

const ThemeProviderContext = createContext<ThemeContextValue | undefined>(
  undefined,
);

export function ThemeProvider({
  children,
  defaultTheme = "system",
  storageKey = "cc-switch-theme",
}: ThemeProviderProps) {
  const getInitialTheme = () => {
    if (typeof window === "undefined") {
      return defaultTheme;
    }

    const stored = window.localStorage.getItem(storageKey) as Theme | null;
    if (stored === "light" || stored === "dark" || stored === "system") {
      return stored;
    }

    return defaultTheme;
  };

  const [theme, setThemeState] = useState<Theme>(getInitialTheme);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    window.localStorage.setItem(storageKey, theme);
  }, [theme, storageKey]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    const root = window.document.documentElement;

    if (theme !== "system") {
      root.classList.remove("light", "dark");
      root.classList.add(theme);
      return;
    }

    let isMounted = true;

    const applyTheme = (isDark: boolean) => {
      if (!isMounted) return;
      root.classList.toggle("dark", isDark);
      root.classList.toggle("light", !isDark);
    };

    const syncSystemTheme = async () => {
      if (!isMounted) return;
      try {
        const sysTheme = await invoke<string | null>("get_system_theme");
        if (sysTheme === "dark") {
          applyTheme(true);
          return;
        } else if (sysTheme === "light") {
          applyTheme(false);
          return;
        }
      } catch (e) {
        console.debug("Failed to read system theme via get_system_theme:", e);
      }

      // Fallback to matchMedia
      const isDark =
        window.matchMedia &&
        window.matchMedia("(prefers-color-scheme: dark)").matches;
      applyTheme(isDark);
    };

    // Initial sync
    void syncSystemTheme();

    const mediaQuery = window.matchMedia?.("(prefers-color-scheme: dark)");
    const handleMediaChange = () => {
      void syncSystemTheme();
    };
    mediaQuery?.addEventListener("change", handleMediaChange);

    // Fallback periodic polling for Linux/KDE Portal theme changes
    // which may not emit WebKit mediaQuery events.
    const intervalId = setInterval(syncSystemTheme, 2000);

    return () => {
      isMounted = false;
      clearInterval(intervalId);
      mediaQuery?.removeEventListener("change", handleMediaChange);
    };
  }, [theme]);

  // Sync native window theme (Windows/macOS title bar)
  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    let isCancelled = false;

    const updateNativeTheme = async (nativeTheme: string) => {
      if (isCancelled) return;
      try {
        await invoke("set_window_theme", { theme: nativeTheme });
      } catch (e) {
        // Ignore errors (e.g., when not running in Tauri)
        console.debug("Failed to set native window theme:", e);
      }
    };

    // When "system", pass "system" so Tauri uses None (follows OS theme natively).
    // This keeps the WebView's prefers-color-scheme in sync with the real OS theme,
    // allowing effect #3's media query listener to fire on system theme changes.
    if (theme === "system") {
      updateNativeTheme("system");
    } else {
      updateNativeTheme(theme);
    }

    return () => {
      isCancelled = true;
    };
  }, [theme]);

  const value = useMemo<ThemeContextValue>(
    () => ({
      theme,
      setTheme: (nextTheme: Theme) => {
        if (nextTheme === theme) return;
        setThemeState(nextTheme);
      },
    }),
    [theme],
  );

  return (
    <ThemeProviderContext.Provider value={value}>
      {children}
    </ThemeProviderContext.Provider>
  );
}

export function useTheme() {
  const context = useContext(ThemeProviderContext);
  if (context === undefined) {
    throw new Error("useTheme must be used within a ThemeProvider");
  }
  return context;
}
