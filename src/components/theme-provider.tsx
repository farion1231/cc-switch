import React, {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  CUSTOM_THEME_TOKENS,
  CUSTOM_THEME_VARIABLES,
  DEFAULT_CUSTOM_THEME,
  DEFAULT_THEME_PRESET,
  type CustomThemeConfig,
  type CustomThemeMode,
  type CustomThemePalette,
  type CustomThemeToken,
  type ThemePreset,
  deriveChartPalette,
  deriveStatusPalette,
  hexToHslString,
  isHexColor,
  normalizeHex,
  syncDerivedDarkPalette,
} from "@/lib/theme/customTheme";

type Theme = "light" | "dark" | "system";

function parseStoredCustomTheme(value: string | null): CustomThemeConfig | null {
  if (!value) return null;

  try {
    const parsed = JSON.parse(value) as Partial<CustomThemeConfig>;
    const result = { ...DEFAULT_CUSTOM_THEME.light };
    const palette = parsed.light;

    if (palette) {
      for (const token of CUSTOM_THEME_TOKENS) {
        const candidate = palette[token];
        if (typeof candidate === "string" && isHexColor(normalizeHex(candidate))) {
          result[token] = normalizeHex(candidate);
        }
      }
    }

    return syncDerivedDarkPalette({
      light: result,
      dark: DEFAULT_CUSTOM_THEME.dark,
    });
  } catch {
    return null;
  }
}

interface ThemeProviderProps {
  children: React.ReactNode;
  defaultTheme?: Theme;
  storageKey?: string;
  presetStorageKey?: string;
  customThemeStorageKey?: string;
}

interface ThemeContextValue {
  theme: Theme;
  themePreset: ThemePreset;
  customTheme: CustomThemeConfig;
  setTheme: (theme: Theme, event?: React.MouseEvent) => void;
  setThemePreset: (preset: ThemePreset) => void;
  setCustomThemeColor: (
    mode: CustomThemeMode,
    token: CustomThemeToken,
    color: string,
  ) => void;
  setCustomThemeColors: (
    mode: CustomThemeMode,
    colors: Partial<CustomThemePalette>,
  ) => void;
  resetCustomTheme: (mode?: CustomThemeMode) => void;
}

const ThemeProviderContext = createContext<ThemeContextValue | undefined>(
  undefined,
);

function getSystemThemeMode(): CustomThemeMode {
  if (typeof window === "undefined" || !window.matchMedia) {
    return "light";
  }

  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

export function ThemeProvider({
  children,
  defaultTheme = "system",
  storageKey = "cc-switch-theme",
  presetStorageKey = "cc-switch-theme-preset",
  customThemeStorageKey = "cc-switch-theme-custom",
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
  const getInitialThemePreset = () => {
    if (typeof window === "undefined") {
      return DEFAULT_THEME_PRESET;
    }

    const stored = window.localStorage.getItem(
      presetStorageKey,
    ) as ThemePreset | null;
    if (stored === "default" || stored === "bubblegum" || stored === "custom") {
      return stored;
    }

    return DEFAULT_THEME_PRESET;
  };

  const [themePreset, setThemePresetState] = useState<ThemePreset>(
    getInitialThemePreset,
  );
  const [customTheme, setCustomThemeState] = useState<CustomThemeConfig>(() => {
    if (typeof window === "undefined") {
      return DEFAULT_CUSTOM_THEME;
    }

    return (
      parseStoredCustomTheme(
        window.localStorage.getItem(customThemeStorageKey),
      ) ?? DEFAULT_CUSTOM_THEME
    );
  });
  const [systemThemeMode, setSystemThemeMode] =
    useState<CustomThemeMode>(getSystemThemeMode);
  const resolvedThemeMode: CustomThemeMode =
    theme === "system" ? systemThemeMode : theme;

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

    window.localStorage.setItem(presetStorageKey, themePreset);
  }, [themePreset, presetStorageKey]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    window.localStorage.setItem(
      customThemeStorageKey,
      JSON.stringify(customTheme),
    );
  }, [customTheme, customThemeStorageKey]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    window.document.documentElement.dataset.themePreset = themePreset;
  }, [themePreset]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    const root = window.document.documentElement;

    for (const variable of CUSTOM_THEME_VARIABLES) {
      root.style.removeProperty(variable);
    }

    if (themePreset !== "custom") {
      return;
    }

    const palette =
      resolvedThemeMode === "dark" ? customTheme.dark : customTheme.light;
    const chartPalette = deriveChartPalette(palette, resolvedThemeMode);
    const statusPalette = deriveStatusPalette(palette, resolvedThemeMode);
    root.style.setProperty("--background", hexToHslString(palette.background));
    root.style.setProperty("--foreground", hexToHslString(palette.foreground));
    root.style.setProperty("--card", hexToHslString(palette.card));
    root.style.setProperty(
      "--card-foreground",
      hexToHslString(palette.cardForeground),
    );
    root.style.setProperty("--popover", hexToHslString(palette.popover));
    root.style.setProperty(
      "--popover-foreground",
      hexToHslString(palette.popoverForeground),
    );
    root.style.setProperty("--primary", hexToHslString(palette.primary));
    root.style.setProperty(
      "--primary-foreground",
      hexToHslString(palette.primaryForeground),
    );
    root.style.setProperty("--secondary", hexToHslString(palette.secondary));
    root.style.setProperty(
      "--secondary-foreground",
      hexToHslString(palette.secondaryForeground),
    );
    root.style.setProperty("--muted", hexToHslString(palette.muted));
    root.style.setProperty(
      "--muted-foreground",
      hexToHslString(palette.mutedForeground),
    );
    root.style.setProperty("--accent", hexToHslString(palette.accent));
    root.style.setProperty(
      "--accent-foreground",
      hexToHslString(palette.accentForeground),
    );
    root.style.setProperty(
      "--destructive",
      hexToHslString(palette.destructive),
    );
    root.style.setProperty(
      "--destructive-foreground",
      hexToHslString(palette.destructiveForeground),
    );
    root.style.setProperty("--success", hexToHslString(statusPalette.success));
    root.style.setProperty("--warning", hexToHslString(statusPalette.warning));
    root.style.setProperty("--border", hexToHslString(palette.border));
    root.style.setProperty("--input", hexToHslString(palette.input));
    root.style.setProperty("--ring", hexToHslString(palette.ring));
    root.style.setProperty("--chart-1", hexToHslString(chartPalette.chart1));
    root.style.setProperty("--chart-2", hexToHslString(chartPalette.chart2));
    root.style.setProperty("--chart-3", hexToHslString(chartPalette.chart3));
    root.style.setProperty("--chart-4", hexToHslString(chartPalette.chart4));
    root.style.setProperty("--chart-5", hexToHslString(chartPalette.chart5));
  }, [customTheme, resolvedThemeMode, themePreset]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    const root = window.document.documentElement;
    root.classList.remove("light", "dark");
    root.classList.add(resolvedThemeMode);
  }, [resolvedThemeMode]);

  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) {
      return;
    }

    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const handleChange = () =>
      setSystemThemeMode(mediaQuery.matches ? "dark" : "light");

    handleChange();
    mediaQuery.addEventListener("change", handleChange);
    return () => mediaQuery.removeEventListener("change", handleChange);
  }, []);

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
      themePreset,
      customTheme,
      setTheme: (nextTheme: Theme, event?: React.MouseEvent) => {
        // Skip if same theme
        if (nextTheme === theme) return;

        // Set transition origin coordinates from click event
        const x = event?.clientX ?? window.innerWidth / 2;
        const y = event?.clientY ?? window.innerHeight / 2;
        document.documentElement.style.setProperty(
          "--theme-transition-x",
          `${x}px`,
        );
        document.documentElement.style.setProperty(
          "--theme-transition-y",
          `${y}px`,
        );

        // Use View Transitions API if available, otherwise fall back to instant change
        if (document.startViewTransition) {
          document.startViewTransition(() => {
            setThemeState(nextTheme);
          });
        } else {
          setThemeState(nextTheme);
        }
      },
      setThemePreset: (nextPreset: ThemePreset) => {
        if (nextPreset === themePreset) return;
        setThemePresetState(nextPreset);
      },
      setCustomThemeColor: (
        mode: CustomThemeMode,
        token: CustomThemeToken,
        color: string,
      ) => {
        const normalized = normalizeHex(color);
        if (!isHexColor(normalized)) return;

        setCustomThemeState((current) => {
          const next = {
            ...current,
            [mode]: {
              ...current[mode],
              [token]: normalized,
            },
          };

          return mode === "light" ? syncDerivedDarkPalette(next) : next;
        });
      },
      setCustomThemeColors: (
        mode: CustomThemeMode,
        colors: Partial<CustomThemePalette>,
      ) => {
        const entries = Object.entries(colors).filter(
          ([, value]) =>
            typeof value === "string" && isHexColor(normalizeHex(value)),
        ) as Array<[CustomThemeToken, string]>;

        if (entries.length === 0) return;

        setCustomThemeState((current) => {
          const next = {
            ...current,
            [mode]: {
              ...current[mode],
              ...Object.fromEntries(
                entries.map(([token, value]) => [token, normalizeHex(value)]),
              ),
            },
          };

          return mode === "light" ? syncDerivedDarkPalette(next) : next;
        });
      },
      resetCustomTheme: (mode?: CustomThemeMode) => {
        setCustomThemeState((current) => {
          if (!mode) {
            return DEFAULT_CUSTOM_THEME;
          }

          if (mode === "light") {
            return syncDerivedDarkPalette({
              ...current,
              light: DEFAULT_CUSTOM_THEME.light,
            });
          }

          return {
            ...current,
            [mode]: DEFAULT_CUSTOM_THEME[mode],
          };
        });
      },
    }),
    [customTheme, theme, themePreset],
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
