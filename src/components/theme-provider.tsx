import React, {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";

type Theme = "light" | "dark" | "system";
type ThemePreset = "default" | "bubblegum" | "custom";
type CustomThemeMode = "light" | "dark";
type CustomThemeToken =
  | "background"
  | "foreground"
  | "card"
  | "cardForeground"
  | "popover"
  | "popoverForeground"
  | "primary"
  | "primaryForeground"
  | "secondary"
  | "secondaryForeground"
  | "muted"
  | "mutedForeground"
  | "accent"
  | "accentForeground"
  | "destructive"
  | "destructiveForeground"
  | "border"
  | "input"
  | "ring";

type CustomThemePalette = Record<CustomThemeToken, string>;
type CustomThemeConfig = Record<CustomThemeMode, CustomThemePalette>;

const DEFAULT_THEME_PRESET: ThemePreset = "default";
const CUSTOM_THEME_VARIABLES = [
  "--background",
  "--foreground",
  "--card",
  "--card-foreground",
  "--popover",
  "--popover-foreground",
  "--primary",
  "--primary-foreground",
  "--secondary",
  "--secondary-foreground",
  "--muted",
  "--muted-foreground",
  "--accent",
  "--accent-foreground",
  "--destructive",
  "--destructive-foreground",
  "--success",
  "--warning",
  "--border",
  "--input",
  "--ring",
  "--chart-1",
  "--chart-2",
  "--chart-3",
  "--chart-4",
  "--chart-5",
] as const;
const DEFAULT_CUSTOM_THEME: CustomThemeConfig = {
  light: {
    background: "#ffffff",
    foreground: "#09090b",
    card: "#ffffff",
    cardForeground: "#09090b",
    popover: "#ffffff",
    popoverForeground: "#09090b",
    primary: "#1f9cff",
    primaryForeground: "#ffffff",
    secondary: "#f4f4f5",
    secondaryForeground: "#18181b",
    muted: "#f4f4f5",
    mutedForeground: "#71717a",
    accent: "#f4f4f5",
    accentForeground: "#18181b",
    destructive: "#ef4444",
    destructiveForeground: "#fafafa",
    border: "#e4e4e7",
    input: "#e4e4e7",
    ring: "#1f9cff",
  },
  dark: {
    background: "#1d1d20",
    foreground: "#fafafa",
    card: "#27272b",
    cardForeground: "#fafafa",
    popover: "#27272b",
    popoverForeground: "#fafafa",
    primary: "#1490ff",
    primaryForeground: "#ffffff",
    secondary: "#2d2d31",
    secondaryForeground: "#fafafa",
    muted: "#2d2d31",
    mutedForeground: "#a1a1aa",
    accent: "#2d2d31",
    accentForeground: "#fafafa",
    destructive: "#7f1d1d",
    destructiveForeground: "#fafafa",
    border: "#3c3c40",
    input: "#3c3c40",
    ring: "#1490ff",
  },
};

function isHexColor(value: string): boolean {
  return /^#([0-9a-f]{6}|[0-9a-f]{3})$/i.test(value);
}

function normalizeHex(value: string): string {
  const trimmed = value.trim();
  if (!trimmed.startsWith("#")) {
    return `#${trimmed}`;
  }
  return trimmed;
}

function hexToRgb(hex: string) {
  const normalized = normalizeHex(hex).replace("#", "");
  const fullHex =
    normalized.length === 3
      ? normalized
          .split("")
          .map((char) => `${char}${char}`)
          .join("")
      : normalized;

  const int = Number.parseInt(fullHex, 16);
  return {
    r: (int >> 16) & 255,
    g: (int >> 8) & 255,
    b: int & 255,
  };
}

function hexToHslString(hex: string): string {
  const { h, s, l } = hexToHslValues(hex);
  return `${Math.round(h)} ${Math.round(s * 100)}% ${Math.round(l * 100)}%`;
}

function hexToHslValues(hex: string) {
  const { r, g, b } = hexToRgb(hex);
  const red = r / 255;
  const green = g / 255;
  const blue = b / 255;
  const max = Math.max(red, green, blue);
  const min = Math.min(red, green, blue);
  let hue = 0;
  let saturation = 0;
  const lightness = (max + min) / 2;

  if (max !== min) {
    const delta = max - min;
    saturation =
      lightness > 0.5 ? delta / (2 - max - min) : delta / (max + min);
    switch (max) {
      case red:
        hue = (green - blue) / delta + (green < blue ? 6 : 0);
        break;
      case green:
        hue = (blue - red) / delta + 2;
        break;
      default:
        hue = (red - green) / delta + 4;
        break;
    }
    hue /= 6;
  }

  return { h: hue * 360, s: saturation, l: lightness };
}

function hslToHex(h: number, s: number, l: number): string {
  const hue = ((h % 360) + 360) % 360;
  const saturation = Math.max(0, Math.min(1, s));
  const lightness = Math.max(0, Math.min(1, l));
  const chroma = (1 - Math.abs(2 * lightness - 1)) * saturation;
  const x = chroma * (1 - Math.abs(((hue / 60) % 2) - 1));
  const m = lightness - chroma / 2;

  let red = 0;
  let green = 0;
  let blue = 0;

  if (hue < 60) {
    red = chroma;
    green = x;
  } else if (hue < 120) {
    red = x;
    green = chroma;
  } else if (hue < 180) {
    green = chroma;
    blue = x;
  } else if (hue < 240) {
    green = x;
    blue = chroma;
  } else if (hue < 300) {
    red = x;
    blue = chroma;
  } else {
    red = chroma;
    blue = x;
  }

  const toHex = (value: number) =>
    Math.round((value + m) * 255)
      .toString(16)
      .padStart(2, "0");

  return `#${toHex(red)}${toHex(green)}${toHex(blue)}`;
}

function remapForDark(
  hex: string,
  targetLightness: number,
  options: { satScale?: number; minSat?: number; maxSat?: number } = {},
) {
  const { h, s } = hexToHslValues(hex);
  const scaledSaturation = s * (options.satScale ?? 1);
  const nextSaturation = Math.min(
    options.maxSat ?? 1,
    Math.max(options.minSat ?? 0, scaledSaturation),
  );
  return hslToHex(h, nextSaturation, targetLightness);
}

function mixHex(base: string, target: string, ratio: number): string {
  const a = hexToRgb(base);
  const b = hexToRgb(target);
  const channel = (start: number, end: number) =>
    Math.round(start + (end - start) * ratio)
      .toString(16)
      .padStart(2, "0");

  return `#${channel(a.r, b.r)}${channel(a.g, b.g)}${channel(a.b, b.b)}`;
}

function getReadableTextColor(hex: string): string {
  const { r, g, b } = hexToRgb(hex);
  const toLinear = (channel: number) => {
    const value = channel / 255;
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  };

  const luminance =
    0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);

  return luminance > 0.58 ? "#111827" : "#ffffff";
}

function deriveDarkPalette(base: CustomThemePalette): CustomThemePalette {
  const background = remapForDark(base.background, 0.12, {
    satScale: 0.3,
    maxSat: 0.18,
  });
  const foreground = getReadableTextColor(background);
  const card = remapForDark(base.card, 0.16, {
    satScale: 0.35,
    maxSat: 0.2,
  });
  const cardForeground = getReadableTextColor(card);
  const popover = remapForDark(base.popover, 0.16, {
    satScale: 0.35,
    maxSat: 0.2,
  });
  const popoverForeground = getReadableTextColor(popover);
  const primaryBase = hexToHslValues(base.primary);
  const primary = hslToHex(
    primaryBase.h,
    Math.min(0.95, Math.max(0.5, primaryBase.s * 1.05)),
    Math.min(0.68, Math.max(0.58, primaryBase.l * 0.92)),
  );
  const primaryForeground = getReadableTextColor(primary);
  const secondary = remapForDark(base.secondary, 0.22, {
    satScale: 0.5,
    maxSat: 0.28,
  });
  const secondaryForeground = getReadableTextColor(secondary);
  const muted = remapForDark(base.muted, 0.22, {
    satScale: 0.35,
    maxSat: 0.22,
  });
  const mutedForeground = mixHex(foreground, background, 0.32);
  const accent = remapForDark(base.accent, 0.24, {
    satScale: 0.55,
    maxSat: 0.32,
  });
  const accentForeground = getReadableTextColor(accent);
  const destructive = remapForDark(base.destructive, 0.34, {
    satScale: 0.85,
    minSat: 0.45,
    maxSat: 0.78,
  });
  const destructiveForeground = getReadableTextColor(destructive);
  const border = remapForDark(base.border, 0.26, {
    satScale: 0.25,
    maxSat: 0.16,
  });
  const input = remapForDark(base.input, 0.26, {
    satScale: 0.25,
    maxSat: 0.16,
  });
  const ringBase = hexToHslValues(base.ring);
  const ring = hslToHex(
    ringBase.h,
    Math.min(0.92, Math.max(0.45, ringBase.s)),
    Math.min(0.7, Math.max(0.58, ringBase.l * 0.96)),
  );

  return {
    background,
    foreground,
    card,
    cardForeground,
    popover,
    popoverForeground,
    primary,
    primaryForeground,
    secondary,
    secondaryForeground,
    muted,
    mutedForeground,
    accent,
    accentForeground,
    destructive,
    destructiveForeground,
    border,
    input,
    ring,
  };
}

function shiftChartHue(
  hex: string,
  hueShift: number,
  targetLightness: number,
  options: { satScale?: number; minSat?: number; maxSat?: number } = {},
) {
  const { h, s } = hexToHslValues(hex);
  const scaledSaturation = s * (options.satScale ?? 1);
  const nextSaturation = Math.min(
    options.maxSat ?? 1,
    Math.max(options.minSat ?? 0, scaledSaturation),
  );
  return hslToHex(h + hueShift, nextSaturation, targetLightness);
}

function deriveChartPalette(
  palette: CustomThemePalette,
  mode: CustomThemeMode,
) {
  return {
    chart1: palette.primary,
    chart2: shiftChartHue(
      palette.primary,
      115,
      mode === "dark" ? 0.5 : 0.42,
      {
        satScale: 0.95,
        minSat: 0.42,
        maxSat: 0.9,
      },
    ),
    chart3: shiftChartHue(
      palette.primary,
      38,
      mode === "dark" ? 0.6 : 0.52,
      {
        satScale: 1.08,
        minSat: 0.46,
        maxSat: 0.95,
      },
    ),
    chart4: shiftChartHue(
      palette.primary,
      72,
      mode === "dark" ? 0.7 : 0.6,
      {
        satScale: 0.92,
        minSat: 0.42,
        maxSat: 0.88,
      },
    ),
    chart5: palette.destructive,
  };
}

function deriveStatusPalette(
  palette: CustomThemePalette,
  mode: CustomThemeMode,
) {
  const chartPalette = deriveChartPalette(palette, mode);
  return {
    success: chartPalette.chart2,
    warning: chartPalette.chart3,
  };
}

function parseStoredCustomTheme(
  value: string | null,
): CustomThemeConfig | null {
  if (!value) return null;

  try {
    const parsed = JSON.parse(value) as Partial<CustomThemeConfig>;
    const modes: CustomThemeMode[] = ["light", "dark"];
    const tokens: CustomThemeToken[] = [
      "background",
      "foreground",
      "card",
      "cardForeground",
      "popover",
      "popoverForeground",
      "primary",
      "primaryForeground",
      "secondary",
      "secondaryForeground",
      "muted",
      "mutedForeground",
      "accent",
      "accentForeground",
      "destructive",
      "destructiveForeground",
      "border",
      "input",
      "ring",
    ];

    const result = structuredClone(DEFAULT_CUSTOM_THEME);
    for (const mode of modes) {
      const palette = parsed[mode];
      if (!palette) continue;
      for (const token of tokens) {
        const candidate = palette[token];
        if (
          typeof candidate === "string" &&
          isHexColor(normalizeHex(candidate))
        ) {
          result[mode][token] = normalizeHex(candidate);
        }
      }
    }

    return result;
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
      resolvedThemeMode === "dark"
        ? deriveDarkPalette(customTheme.light)
        : customTheme.light;
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

        setCustomThemeState((current) => ({
          ...current,
          [mode]: {
            ...current[mode],
            [token]: normalized,
          },
        }));
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

        setCustomThemeState((current) => ({
          ...current,
          [mode]: {
            ...current[mode],
            ...Object.fromEntries(
              entries.map(([token, value]) => [token, normalizeHex(value)]),
            ),
          },
        }));
      },
      resetCustomTheme: (mode?: CustomThemeMode) => {
        setCustomThemeState((current) => {
          if (!mode) {
            return DEFAULT_CUSTOM_THEME;
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
