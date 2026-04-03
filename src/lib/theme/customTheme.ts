export type ThemePreset = "default" | "bubblegum" | "custom";
export type CustomThemeMode = "light" | "dark";

export const CUSTOM_THEME_TOKENS = [
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
  "success",
  "info",
  "warning",
  "error",
  "border",
  "input",
  "ring",
] as const;

export type CustomThemeToken = (typeof CUSTOM_THEME_TOKENS)[number];
export type CustomThemePalette = Record<CustomThemeToken, string>;
export type CustomThemeConfig = Record<CustomThemeMode, CustomThemePalette>;

export const CUSTOM_THEME_VARIABLES = [
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
  "--info",
  "--warning",
  "--error",
  "--border",
  "--input",
  "--ring",
  "--chart-1",
  "--chart-2",
  "--chart-3",
  "--chart-4",
  "--chart-5",
] as const;

export const DEFAULT_THEME_PRESET: ThemePreset = "default";

const DEFAULT_LIGHT_CUSTOM_THEME: CustomThemePalette = {
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
  success: "#16a34a",
  info: "#2563eb",
  warning: "#d97706",
  error: "#ef4444",
  border: "#e4e4e7",
  input: "#e4e4e7",
  ring: "#1f9cff",
};

export function normalizeHex(value: string): string {
  const trimmed = value.trim();
  return trimmed.startsWith("#") ? trimmed : `#${trimmed}`;
}

export function isHexColor(value: string): boolean {
  return /^#([0-9a-f]{6}|[0-9a-f]{3})$/i.test(value);
}

export function hexToRgb(hex: string) {
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

export function getReadableTextColor(hex: string): string {
  const { r, g, b } = hexToRgb(hex);
  const toLinear = (channel: number) => {
    const value = channel / 255;
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  };

  const luminance =
    0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);

  return luminance > 0.58 ? "#111827" : "#ffffff";
}

export function mixHex(base: string, target: string, ratio: number): string {
  const a = hexToRgb(base);
  const b = hexToRgb(target);
  const channel = (start: number, end: number) =>
    Math.round(start + (end - start) * ratio)
      .toString(16)
      .padStart(2, "0");

  return `#${channel(a.r, b.r)}${channel(a.g, b.g)}${channel(a.b, b.b)}`;
}

export function hexToHslValues(hex: string) {
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

export function hexToHslString(hex: string): string {
  const { h, s, l } = hexToHslValues(hex);
  return `${Math.round(h)} ${Math.round(s * 100)}% ${Math.round(l * 100)}%`;
}

export function hslToHex(h: number, s: number, l: number): string {
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

export function remapForDark(
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

export function rgbToHex(r: number, g: number, b: number): string {
  return `#${[r, g, b]
    .map((channel) => Math.max(0, Math.min(255, Math.round(channel))))
    .map((channel) => channel.toString(16).padStart(2, "0"))
    .join("")}`;
}

function rgbToHsv(r: number, g: number, b: number) {
  const red = r / 255;
  const green = g / 255;
  const blue = b / 255;
  const max = Math.max(red, green, blue);
  const min = Math.min(red, green, blue);
  const delta = max - min;
  let hue = 0;

  if (delta !== 0) {
    if (max === red) hue = ((green - blue) / delta) % 6;
    else if (max === green) hue = (blue - red) / delta + 2;
    else hue = (red - green) / delta + 4;
  }

  hue = Math.round(hue * 60);
  if (hue < 0) hue += 360;

  return {
    h: hue,
    s: max === 0 ? 0 : delta / max,
    v: max,
  };
}

export function hexToHsv(hex: string) {
  const { r, g, b } = hexToRgb(hex);
  return rgbToHsv(r, g, b);
}

export function hsvToHex(h: number, s: number, v: number) {
  const hue = ((h % 360) + 360) % 360;
  const chroma = v * s;
  const x = chroma * (1 - Math.abs(((hue / 60) % 2) - 1));
  const m = v - chroma;

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

  return rgbToHex((red + m) * 255, (green + m) * 255, (blue + m) * 255);
}

export function deriveDarkPalette(
  base: CustomThemePalette,
): CustomThemePalette {
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
  const success = remapForDark(base.success, 0.56, {
    satScale: 0.92,
    minSat: 0.46,
    maxSat: 0.82,
  });
  const info = remapForDark(base.info, 0.62, {
    satScale: 0.96,
    minSat: 0.44,
    maxSat: 0.86,
  });
  const warning = remapForDark(base.warning, 0.62, {
    satScale: 0.98,
    minSat: 0.5,
    maxSat: 0.9,
  });
  const error = remapForDark(base.error, 0.62, {
    satScale: 0.94,
    minSat: 0.5,
    maxSat: 0.88,
  });
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
    success,
    info,
    warning,
    error,
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

export function deriveChartPalette(
  palette: CustomThemePalette,
  mode: CustomThemeMode,
) {
  return {
    chart1: palette.primary,
    chart2: shiftChartHue(palette.primary, 115, mode === "dark" ? 0.5 : 0.42, {
      satScale: 0.95,
      minSat: 0.42,
      maxSat: 0.9,
    }),
    chart3: shiftChartHue(palette.primary, 38, mode === "dark" ? 0.6 : 0.52, {
      satScale: 1.08,
      minSat: 0.46,
      maxSat: 0.95,
    }),
    chart4: shiftChartHue(palette.primary, 72, mode === "dark" ? 0.7 : 0.6, {
      satScale: 0.92,
      minSat: 0.42,
      maxSat: 0.88,
    }),
    chart5: palette.destructive,
  };
}

export function deriveStatusPalette(
  palette: CustomThemePalette,
  _mode: CustomThemeMode,
) {
  return {
    success: palette.success,
    warning: palette.warning,
  };
}

export function createCustomThemeConfig(
  lightPalette: CustomThemePalette,
): CustomThemeConfig {
  const nextLight = { ...lightPalette };
  return {
    light: nextLight,
    dark: deriveDarkPalette(nextLight),
  };
}

function getExplicitDarkOverrides(
  config: CustomThemeConfig,
): Partial<CustomThemePalette> {
  const derivedDark = deriveDarkPalette(config.light);
  const overrides: Partial<CustomThemePalette> = {};

  for (const token of CUSTOM_THEME_TOKENS) {
    if (config.dark[token].toLowerCase() !== derivedDark[token].toLowerCase()) {
      overrides[token] = config.dark[token];
    }
  }

  return overrides;
}

export function syncDerivedDarkPalette(
  config: CustomThemeConfig,
  options?: {
    preserveDarkOverrides?: boolean;
    sourceConfig?: CustomThemeConfig;
  },
): CustomThemeConfig {
  const next = createCustomThemeConfig(config.light);

  if (!options?.preserveDarkOverrides) {
    return next;
  }

  const overrides = getExplicitDarkOverrides(options.sourceConfig ?? config);
  if (Object.keys(overrides).length === 0) {
    return next;
  }

  return {
    light: next.light,
    dark: {
      ...next.dark,
      ...overrides,
    },
  };
}

export const DEFAULT_CUSTOM_THEME = createCustomThemeConfig(
  DEFAULT_LIGHT_CUSTOM_THEME,
);
