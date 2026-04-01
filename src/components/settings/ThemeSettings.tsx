import {
  ChevronDown,
  ChevronRight,
  Monitor,
  Moon,
  Search,
  Sparkles,
  Sun,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useTheme } from "@/components/theme-provider";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

const CUSTOM_THEME_FIELDS = [
  { key: "background", labelKey: "settings.themeColorBackground" },
  { key: "foreground", labelKey: "settings.themeColorForeground" },
  { key: "primary", labelKey: "settings.themeColorBackground" },
  { key: "primaryForeground", labelKey: "settings.themeColorForeground" },
  { key: "secondary", labelKey: "settings.themeColorBackground" },
  { key: "secondaryForeground", labelKey: "settings.themeColorForeground" },
  { key: "accent", labelKey: "settings.themeColorBackground" },
  { key: "accentForeground", labelKey: "settings.themeColorForeground" },
  { key: "card", labelKey: "settings.themeColorBackground" },
  { key: "cardForeground", labelKey: "settings.themeColorForeground" },
  { key: "popover", labelKey: "settings.themeColorBackground" },
  { key: "popoverForeground", labelKey: "settings.themeColorForeground" },
  { key: "muted", labelKey: "settings.themeColorBackground" },
  { key: "mutedForeground", labelKey: "settings.themeColorForeground" },
  { key: "destructive", labelKey: "settings.themeColorBackground" },
  { key: "destructiveForeground", labelKey: "settings.themeColorForeground" },
  { key: "border", labelKey: "settings.themeColorBorder" },
  { key: "input", labelKey: "settings.themeColorInput" },
  { key: "ring", labelKey: "settings.themeColorRing" },
] as const;
const SECTION_DEFINITIONS = [
  {
    id: "primary",
    titleKey: "settings.themeSectionPrimary",
    rowKeys: ["primary", "primaryForeground"] as const,
  },
  {
    id: "secondary",
    titleKey: "settings.themeSectionSecondary",
    rowKeys: ["secondary", "secondaryForeground"] as const,
  },
  {
    id: "accent",
    titleKey: "settings.themeSectionAccent",
    rowKeys: ["accent", "accentForeground"] as const,
  },
  {
    id: "base",
    titleKey: "settings.themeSectionBase",
    rowKeys: ["background", "foreground"] as const,
  },
  {
    id: "card",
    titleKey: "settings.themeSectionCard",
    rowKeys: ["card", "cardForeground"] as const,
  },
  {
    id: "popover",
    titleKey: "settings.themeSectionPopover",
    rowKeys: ["popover", "popoverForeground"] as const,
  },
  {
    id: "muted",
    titleKey: "settings.themeSectionMuted",
    rowKeys: ["muted", "mutedForeground"] as const,
  },
  {
    id: "destructive",
    titleKey: "settings.themeSectionDestructive",
    rowKeys: ["destructive", "destructiveForeground"] as const,
  },
  {
    id: "borderInput",
    titleKey: "settings.themeSectionBorderInput",
    rowKeys: ["border", "input", "ring"] as const,
  },
] as const;
const QUICK_COLOR_SWATCHES = [
  "#ffffff",
  "#f4f4f5",
  "#e4e4e7",
  "#a1a1aa",
  "#18181b",
  "#000000",
  "#ef4444",
  "#f97316",
  "#eab308",
  "#22c55e",
  "#14b8a6",
  "#0ea5e9",
  "#6366f1",
  "#8b5cf6",
  "#d946ef",
  "#ec4899",
] as const;

type EditablePaletteMode = "light" | "dark";
type CustomThemeFieldKey = (typeof CUSTOM_THEME_FIELDS)[number]["key"];
type ThemeSectionId = (typeof SECTION_DEFINITIONS)[number]["id"];
type CustomThemePalette = Record<CustomThemeFieldKey, string>;

function normalizeHex(value: string): string {
  const trimmed = value.trim();
  return trimmed.startsWith("#") ? trimmed : `#${trimmed}`;
}

function isHexColor(value: string): boolean {
  return /^#([0-9a-f]{6}|[0-9a-f]{3})$/i.test(value);
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

function readableText(hex: string) {
  const { r, g, b } = hexToRgb(hex);
  const toLinear = (channel: number) => {
    const value = channel / 255;
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  };

  const luminance =
    0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);

  return luminance > 0.58 ? "#111827" : "#ffffff";
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

  return rgbToHex((red + m) * 255, (green + m) * 255, (blue + m) * 255);
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

function rgbToHex(r: number, g: number, b: number): string {
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

  const saturation = max === 0 ? 0 : delta / max;
  const value = max;
  return { h: hue, s: saturation, v: value };
}

function hexToHsv(hex: string) {
  const { r, g, b } = hexToRgb(hex);
  return rgbToHsv(r, g, b);
}

function hsvToHex(h: number, s: number, v: number) {
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

function deriveDarkPalette(base: CustomThemePalette): CustomThemePalette {
  const background = remapForDark(base.background, 0.12, {
    satScale: 0.3,
    maxSat: 0.18,
  });
  const foreground = readableText(background);
  const card = remapForDark(base.card, 0.16, {
    satScale: 0.35,
    maxSat: 0.2,
  });
  const cardForeground = readableText(card);
  const popover = remapForDark(base.popover, 0.16, {
    satScale: 0.35,
    maxSat: 0.2,
  });
  const popoverForeground = readableText(popover);
  const primaryBase = hexToHslValues(base.primary);
  const primary = hslToHex(
    primaryBase.h,
    Math.min(0.95, Math.max(0.5, primaryBase.s * 1.05)),
    Math.min(0.68, Math.max(0.58, primaryBase.l * 0.92)),
  );
  const primaryForeground = readableText(primary);
  const secondary = remapForDark(base.secondary, 0.22, {
    satScale: 0.5,
    maxSat: 0.28,
  });
  const secondaryForeground = readableText(secondary);
  const muted = remapForDark(base.muted, 0.22, {
    satScale: 0.35,
    maxSat: 0.22,
  });
  const mutedForeground = mixHex(foreground, background, 0.32);
  const accent = remapForDark(base.accent, 0.24, {
    satScale: 0.55,
    maxSat: 0.32,
  });
  const accentForeground = readableText(accent);
  const destructive = remapForDark(base.destructive, 0.34, {
    satScale: 0.85,
    minSat: 0.45,
    maxSat: 0.78,
  });
  const destructiveForeground = readableText(destructive);
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

function getFieldLabelKey(fieldKey: CustomThemeFieldKey) {
  return (
    CUSTOM_THEME_FIELDS.find((field) => field.key === fieldKey)?.labelKey ??
    "settings.themeColorBackground"
  );
}

function getAutoColorForField(
  fieldKey: CustomThemeFieldKey,
  palette: CustomThemePalette,
) {
  switch (fieldKey) {
    case "foreground":
      return readableText(palette.background);
    case "primaryForeground":
      return readableText(palette.primary);
    case "secondaryForeground":
      return readableText(palette.secondary);
    case "accentForeground":
      return readableText(palette.accent);
    case "cardForeground":
      return readableText(palette.card);
    case "popoverForeground":
      return readableText(palette.popover);
    case "mutedForeground":
      return readableText(palette.muted);
    case "destructiveForeground":
      return readableText(palette.destructive);
    case "input":
      return palette.border;
    case "ring":
      return palette.primary;
    default:
      return null;
  }
}

function getSectionSyncValues(
  sectionId: ThemeSectionId,
  palette: CustomThemePalette,
): Partial<CustomThemePalette> {
  switch (sectionId) {
    case "primary":
      return { primaryForeground: readableText(palette.primary) };
    case "secondary":
      return { secondaryForeground: readableText(palette.secondary) };
    case "accent":
      return { accentForeground: readableText(palette.accent) };
    case "base":
      return { foreground: readableText(palette.background) };
    case "card":
      return { cardForeground: readableText(palette.card) };
    case "popover":
      return { popoverForeground: readableText(palette.popover) };
    case "muted":
      return { mutedForeground: readableText(palette.muted) };
    case "destructive":
      return { destructiveForeground: readableText(palette.destructive) };
    case "borderInput":
      return {
        input: palette.border,
        ring: palette.primary,
      };
  }
}

export function ThemeSettings() {
  const { t } = useTranslation();
  const {
    theme,
    themePreset,
    customTheme,
    setTheme,
    setThemePreset,
    setCustomThemeColor,
    setCustomThemeColors,
    resetCustomTheme,
  } = useTheme();
  const [systemPrefersDark, setSystemPrefersDark] = useState(() => {
    if (typeof window === "undefined" || !window.matchMedia) {
      return false;
    }
    return window.matchMedia("(prefers-color-scheme: dark)").matches;
  });
  const [customEditorOpen, setCustomEditorOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [openSections, setOpenSections] = useState<
    Record<ThemeSectionId, boolean>
  >({
    primary: false,
    secondary: false,
    accent: false,
    base: false,
    card: false,
    popover: false,
    muted: false,
    destructive: false,
    borderInput: false,
  });

  const filteredSections = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    return SECTION_DEFINITIONS.map((section) => {
      const rows = section.rowKeys.filter((fieldKey) => {
        if (!query) return true;
        const sectionLabel = t(section.titleKey).toLowerCase();
        const fieldLabel = t(getFieldLabelKey(fieldKey)).toLowerCase();
        return (
          sectionLabel.includes(query) ||
          fieldLabel.includes(query) ||
          fieldKey.toLowerCase().includes(query)
        );
      });
      return { ...section, rows };
    }).filter((section) => section.rows.length > 0);
  }, [searchQuery, t]);

  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) {
      return;
    }

    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const handleChange = (event: MediaQueryListEvent) =>
      setSystemPrefersDark(event.matches);

    setSystemPrefersDark(mediaQuery.matches);
    mediaQuery.addEventListener("change", handleChange);
    return () => mediaQuery.removeEventListener("change", handleChange);
  }, []);

  const resolvedPaletteMode: EditablePaletteMode =
    theme === "system" ? (systemPrefersDark ? "dark" : "light") : theme;
  const currentPalette = customTheme.light;
  const previewPalette =
    resolvedPaletteMode === "dark"
      ? deriveDarkPalette(currentPalette)
      : currentPalette;

  return (
    <section className="space-y-4">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("settings.theme")}</h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.themeHint")}
        </p>
      </header>
      <div className="space-y-3">
        <div className="inline-flex gap-1 rounded-xl border border-border-default bg-background p-1">
          <ThemeButton
            active={theme === "light"}
            onClick={(e) => setTheme("light", e)}
            icon={Sun}
          >
            {t("settings.themeLight")}
          </ThemeButton>
          <ThemeButton
            active={theme === "dark"}
            onClick={(e) => setTheme("dark", e)}
            icon={Moon}
          >
            {t("settings.themeDark")}
          </ThemeButton>
          <ThemeButton
            active={theme === "system"}
            onClick={(e) => setTheme("system", e)}
            icon={Monitor}
          >
            {t("settings.themeSystem")}
          </ThemeButton>
        </div>

        <div className="space-y-1.5">
          <h4 className="text-sm font-medium text-foreground">
            {t("settings.themePreset")}
          </h4>
          <p className="text-xs text-muted-foreground">
            {t("settings.themePresetHint")}
          </p>
          <div className="grid gap-2 md:grid-cols-3">
            <PresetCard
              active={themePreset === "default"}
              name={t("settings.themePresetDefault")}
              description={t("settings.themePresetDefaultDescription")}
              swatches={["bg-white", "bg-sky-500", "bg-slate-100"]}
              onClick={() => setThemePreset("default")}
            />
            <PresetCard
              active={themePreset === "bubblegum"}
              name={t("settings.themePresetBubblegum")}
              description={t("settings.themePresetBubblegumDescription")}
              swatches={["bg-rose-50", "bg-pink-500", "bg-amber-200"]}
              onClick={() => setThemePreset("bubblegum")}
            />
            <PresetCard
              active={themePreset === "custom"}
              name={t("settings.themePresetCustom")}
              description={t("settings.themePresetCustomDescription")}
              swatches={[
                customTheme.light.background,
                customTheme.light.primary,
                customTheme.light.accent,
              ]}
              onClick={() => setThemePreset("custom")}
              useInlineSwatches
            />
          </div>
        </div>

        {themePreset === "custom" && (
          <div className="space-y-3">
            <div className="rounded-2xl border border-border/70 bg-card/55 p-4 shadow-sm">
              <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
                <div className="space-y-1">
                  <h4 className="text-sm font-medium text-foreground">
                    {t("settings.themeCustomPalette")}
                  </h4>
                  <p className="text-xs text-muted-foreground">
                    {t("settings.themeCustomPaletteHintCollapsed", {
                      mode:
                        resolvedPaletteMode === "light"
                          ? t("settings.themePaletteLight")
                          : t("settings.themePaletteDark"),
                    })}
                  </p>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <span className="rounded-full border border-border/70 bg-background px-3 py-1 text-xs text-muted-foreground">
                    {t("settings.themeEditingBasePalette")}
                  </span>
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => setCustomEditorOpen((current) => !current)}
                  >
                    {customEditorOpen
                      ? t("settings.themeEditorCollapse")
                      : t("settings.themeEditorExpand")}
                  </Button>
                </div>
              </div>
            </div>

            <Collapsible
              open={customEditorOpen}
              onOpenChange={setCustomEditorOpen}
            >
              <CollapsibleContent className="space-y-0">
                <div className="grid gap-3 xl:grid-cols-[minmax(0,1.08fr)_minmax(320px,0.92fr)]">
                  <div className="rounded-2xl border border-border/70 bg-card/55 p-3 shadow-sm">
                    <div className="space-y-2.5">
                      <div className="flex flex-col gap-2 lg:flex-row lg:items-center lg:justify-between">
                        <div className="space-y-1">
                          <h4 className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
                            {t("settings.themeCustomPalette")}
                          </h4>
                          <p className="text-xs text-muted-foreground">
                            {t("settings.themeCustomPaletteHintSingle")}
                          </p>
                        </div>
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="rounded-full border border-border/70 bg-background px-3 py-1 text-xs text-muted-foreground">
                            {t("settings.themeDarkOverlayBadge")}
                          </span>
                          <Button
                            type="button"
                            variant="outline"
                            size="sm"
                            onClick={() => resetCustomTheme("light")}
                          >
                            {t("settings.themeCustomPaletteResetBase")}
                          </Button>
                        </div>
                      </div>

                      <div className="relative">
                        <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                        <Input
                          value={searchQuery}
                          onChange={(event) =>
                            setSearchQuery(event.target.value)
                          }
                          placeholder={t(
                            "settings.themeEditorSearchPlaceholder",
                          )}
                          className="h-10 rounded-xl pl-10"
                        />
                      </div>

                      <div className="rounded-2xl border border-border/70 bg-background/60">
                        <TooltipProvider delayDuration={250}>
                          <ScrollArea className="h-[520px]">
                            <div className="space-y-2 p-2.5">
                              {filteredSections.length === 0 && (
                                <div className="rounded-xl border border-dashed border-border/70 bg-muted/30 px-4 py-8 text-center text-sm text-muted-foreground">
                                  {t("settings.themeEditorNoResults")}
                                </div>
                              )}

                              {filteredSections.map((section) => {
                                const forcedOpen =
                                  searchQuery.trim().length > 0;
                                const isOpen =
                                  forcedOpen || openSections[section.id];
                                const previewColors = section.rows
                                  .map((fieldKey) => currentPalette[fieldKey])
                                  .slice(0, 3);
                                return (
                                  <Collapsible
                                    key={section.id}
                                    open={isOpen}
                                    onOpenChange={(nextOpen) =>
                                      setOpenSections((current) => ({
                                        ...current,
                                        [section.id]: nextOpen,
                                      }))
                                    }
                                  >
                                    <div className="rounded-xl border border-border/60 bg-background/85 px-2.5 py-2 shadow-sm">
                                      <div className="flex items-center gap-3">
                                        <CollapsibleTrigger asChild>
                                          <button
                                            type="button"
                                            className="flex w-full min-w-0 flex-1 items-center gap-3 rounded-lg px-1 py-1 text-left transition-colors hover:text-foreground"
                                          >
                                            {isOpen ? (
                                              <ChevronDown className="h-4 w-4 shrink-0 text-muted-foreground" />
                                            ) : (
                                              <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" />
                                            )}
                                            <div className="min-w-0 flex-1">
                                              <div className="text-sm font-semibold text-foreground">
                                                {t(section.titleKey)}
                                              </div>
                                              <div className="mt-0.5 text-xs text-muted-foreground">
                                                {section.rows.length}{" "}
                                                {t(
                                                  "settings.themeSectionTokens",
                                                )}
                                              </div>
                                            </div>
                                            <div className="flex items-center gap-1.5">
                                              {previewColors.map(
                                                (color, index) => (
                                                  <span
                                                    key={`${section.id}-${index}`}
                                                    className="h-6 w-6 rounded-full border border-border/70 shadow-sm"
                                                    style={{
                                                      backgroundColor: color,
                                                    }}
                                                  />
                                                ),
                                              )}
                                            </div>
                                          </button>
                                        </CollapsibleTrigger>
                                        <Tooltip>
                                          <TooltipTrigger asChild>
                                            <Button
                                              type="button"
                                              variant="outline"
                                              size="icon"
                                              className="h-9 w-9 shrink-0 rounded-lg border-border/70 text-muted-foreground hover:bg-muted hover:text-foreground"
                                              aria-label={t(
                                                "settings.themeSyncSection",
                                              )}
                                              onClick={() =>
                                                setCustomThemeColors(
                                                  "light",
                                                  getSectionSyncValues(
                                                    section.id,
                                                    currentPalette,
                                                  ),
                                                )
                                              }
                                            >
                                              <Sparkles className="h-4 w-4" />
                                            </Button>
                                          </TooltipTrigger>
                                          <TooltipContent side="left">
                                            {t(
                                              "settings.themeSyncSectionTooltip",
                                            )}
                                          </TooltipContent>
                                        </Tooltip>
                                      </div>

                                      <CollapsibleContent className="space-y-1.5 pt-1.5">
                                        {section.rows.map((fieldKey) => (
                                          <ColorRow
                                            key={`light-${fieldKey}`}
                                            label={t(
                                              getFieldLabelKey(fieldKey),
                                            )}
                                            value={currentPalette[fieldKey]}
                                            onChange={(value) =>
                                              setCustomThemeColor(
                                                "light",
                                                fieldKey,
                                                value,
                                              )
                                            }
                                            onAutoApply={() => {
                                              const autoColor =
                                                getAutoColorForField(
                                                  fieldKey,
                                                  currentPalette,
                                                );
                                              if (!autoColor) return;
                                              setCustomThemeColor(
                                                "light",
                                                fieldKey,
                                                autoColor,
                                              );
                                            }}
                                            showAutoAction={
                                              getAutoColorForField(
                                                fieldKey,
                                                currentPalette,
                                              ) !== null
                                            }
                                            autoLabel={t(
                                              "settings.themeAutoSync",
                                            )}
                                            quickColors={QUICK_COLOR_SWATCHES}
                                          />
                                        ))}
                                      </CollapsibleContent>
                                    </div>
                                  </Collapsible>
                                );
                              })}
                            </div>
                          </ScrollArea>
                        </TooltipProvider>
                      </div>

                      <div className="rounded-xl border border-border/60 bg-muted/35 px-3 py-2 text-xs text-muted-foreground">
                        {t("settings.themeCustomPaletteTip")}
                      </div>
                    </div>
                  </div>

                  <ThemePreviewPanel
                    mode={resolvedPaletteMode}
                    palette={previewPalette}
                    t={t}
                  />
                </div>
              </CollapsibleContent>
            </Collapsible>
          </div>
        )}
      </div>
    </section>
  );
}

interface ThemeButtonProps {
  active: boolean;
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
  icon: React.ComponentType<{ className?: string }>;
  children: React.ReactNode;
}

function ThemeButton({
  active,
  onClick,
  icon: Icon,
  children,
}: ThemeButtonProps) {
  return (
    <Button
      type="button"
      onClick={onClick}
      size="sm"
      variant={active ? "default" : "ghost"}
      className={cn(
        "min-w-[96px] gap-1.5",
        active
          ? "shadow-sm"
          : "text-muted-foreground hover:bg-muted hover:text-foreground",
      )}
    >
      <Icon className="h-3.5 w-3.5" />
      {children}
    </Button>
  );
}

interface PresetCardProps {
  active: boolean;
  name: string;
  description: string;
  swatches: string[];
  onClick: () => void;
  useInlineSwatches?: boolean;
}

function PresetCard({
  active,
  name,
  description,
  swatches,
  onClick,
  useInlineSwatches = false,
}: PresetCardProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "rounded-lg border bg-card p-3 text-left transition-colors",
        active
          ? "border-primary ring-1 ring-primary/30"
          : "border-border-default hover:border-border-hover hover:bg-muted/30",
      )}
    >
      <div className="mb-3 flex items-center justify-between gap-4">
        <div className="text-sm font-medium text-foreground">{name}</div>
        <div className="flex items-center gap-2">
          {swatches.map((swatchClass, index) => (
            <span
              key={`${name}-${index}`}
              className={cn(
                "h-5 w-5 rounded-full border border-border/60 shadow-sm",
                !useInlineSwatches && swatchClass,
              )}
              style={
                useInlineSwatches ? { backgroundColor: swatchClass } : undefined
              }
            />
          ))}
        </div>
      </div>
      <div className="space-y-1">
        <p className="text-xs leading-relaxed text-muted-foreground">
          {description}
        </p>
      </div>
    </button>
  );
}

interface ColorRowProps {
  label: string;
  value: string;
  onChange: (value: string) => void;
  onAutoApply: () => void;
  showAutoAction: boolean;
  autoLabel: string;
  quickColors: readonly string[];
}

function ColorRow({
  label,
  value,
  onChange,
  onAutoApply,
  showAutoAction,
  autoLabel,
  quickColors,
}: ColorRowProps) {
  const { t } = useTranslation();
  const [draftValue, setDraftValue] = useState(value);

  useEffect(() => {
    setDraftValue(value);
  }, [value]);

  const commitDraft = () => {
    const normalized = normalizeHex(draftValue);
    if (!isHexColor(normalized)) {
      setDraftValue(value);
      return;
    }

    setDraftValue(normalized);
    onChange(normalized);
  };

  return (
    <div className="grid grid-cols-[auto_minmax(80px,132px)_minmax(0,1fr)_auto] items-center gap-2.5 rounded-xl border border-border/60 bg-background/70 px-2.5 py-2">
      <ColorPopover
        label={label}
        value={value}
        onChange={(nextValue) => {
          setDraftValue(nextValue);
          onChange(nextValue);
        }}
        quickColors={quickColors}
        t={t}
      />
      <div className="min-w-0 text-sm font-medium text-foreground">{label}</div>
      <Input
        value={draftValue}
        onChange={(event) => setDraftValue(event.target.value)}
        onBlur={commitDraft}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            commitDraft();
          }
        }}
        className="h-10 rounded-xl border-border/70 bg-card/40 font-mono text-sm"
      />
      {showAutoAction ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="outline"
              size="icon"
              className="h-9 w-9 rounded-xl"
              aria-label={autoLabel}
              onClick={onAutoApply}
            >
              <Sparkles className="h-4 w-4" />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="left">
            {t("settings.themeAutoSyncTooltip")}
          </TooltipContent>
        </Tooltip>
      ) : (
        <div className="h-9 w-9" />
      )}
    </div>
  );
}

interface ColorPopoverProps {
  label: string;
  value: string;
  onChange: (value: string) => void;
  quickColors: readonly string[];
  t: (key: string) => string;
}

function ColorPopover({
  label,
  value,
  onChange,
  quickColors,
  t,
}: ColorPopoverProps) {
  const squareRef = useRef<HTMLDivElement | null>(null);
  const hueRef = useRef<HTMLDivElement | null>(null);
  const hsv = useMemo(() => hexToHsv(value), [value]);
  const hueColor = useMemo(() => hsvToHex(hsv.h, 1, 1), [hsv.h]);

  const updateFromSquare = (clientX: number, clientY: number) => {
    const rect = squareRef.current?.getBoundingClientRect();
    if (!rect) return;
    const x = Math.max(0, Math.min(rect.width, clientX - rect.left));
    const y = Math.max(0, Math.min(rect.height, clientY - rect.top));
    const saturation = rect.width === 0 ? 0 : x / rect.width;
    const brightness = rect.height === 0 ? 0 : 1 - y / rect.height;
    onChange(hsvToHex(hsv.h, saturation, brightness));
  };

  const updateFromHue = (clientX: number) => {
    const rect = hueRef.current?.getBoundingClientRect();
    if (!rect) return;
    const x = Math.max(0, Math.min(rect.width, clientX - rect.left));
    const hue = rect.width === 0 ? 0 : (x / rect.width) * 360;
    onChange(hsvToHex(hue, hsv.s, hsv.v));
  };

  const startDrag = (
    event: React.PointerEvent,
    updater: (clientX: number, clientY: number) => void,
  ) => {
    event.preventDefault();
    updater(event.clientX, event.clientY);

    const handleMove = (moveEvent: PointerEvent) =>
      updater(moveEvent.clientX, moveEvent.clientY);
    const handleUp = () => {
      window.removeEventListener("pointermove", handleMove);
      window.removeEventListener("pointerup", handleUp);
    };

    window.addEventListener("pointermove", handleMove);
    window.addEventListener("pointerup", handleUp);
  };

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="h-10 w-10 rounded-2xl border border-border/70 shadow-sm transition-transform hover:scale-[1.02]"
          style={{ backgroundColor: value }}
          aria-label={label}
        />
      </PopoverTrigger>
      <PopoverContent className="w-[22rem] rounded-2xl border-border/70 p-4">
        <div className="space-y-4">
          <div className="flex items-center justify-between gap-3">
            <div>
              <div className="text-sm font-semibold text-foreground">
                {label}
              </div>
              <div className="text-xs text-muted-foreground">
                {value.toUpperCase()}
              </div>
            </div>
            <div
              className="h-12 w-12 rounded-2xl border border-border/70 shadow-sm"
              style={{ backgroundColor: value }}
            />
          </div>

          <label className="block space-y-2">
            <span className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
              {t("settings.themeColorHex")}
            </span>
            <Input
              value={value}
              onChange={(event) => {
                const nextValue = normalizeHex(event.target.value);
                if (isHexColor(nextValue)) {
                  onChange(nextValue);
                }
              }}
              className="h-10 rounded-xl font-mono"
            />
          </label>

          <div className="space-y-3">
            <div className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
              {t("settings.themeColorField")}
            </div>
            <div
              ref={squareRef}
              className="relative h-44 w-full cursor-crosshair overflow-hidden rounded-2xl border border-border/70"
              style={{ backgroundColor: hueColor }}
              onPointerDown={(event) => startDrag(event, updateFromSquare)}
            >
              <div className="absolute inset-0 bg-gradient-to-r from-white via-transparent to-transparent" />
              <div className="absolute inset-0 bg-gradient-to-t from-black via-transparent to-transparent" />
              <div
                className="absolute h-4 w-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow-[0_0_0_1px_rgba(17,24,39,0.32)]"
                style={{
                  left: `${hsv.s * 100}%`,
                  top: `${(1 - hsv.v) * 100}%`,
                  backgroundColor: value,
                }}
              />
            </div>

            <div className="space-y-2">
              <div className="flex items-center justify-between gap-2">
                <span className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
                  {t("settings.themeColorHue")}
                </span>
                <span className="font-mono text-xs text-muted-foreground">
                  {Math.round(hsv.h)}
                </span>
              </div>
              <div
                ref={hueRef}
                className="relative h-4 w-full cursor-ew-resize rounded-full border border-border/70"
                style={{
                  background:
                    "linear-gradient(90deg, #ff0000 0%, #ffff00 16.6%, #00ff00 33.3%, #00ffff 50%, #0000ff 66.6%, #ff00ff 83.3%, #ff0000 100%)",
                }}
                onPointerDown={(event) =>
                  startDrag(event, (clientX) => updateFromHue(clientX))
                }
              >
                <div
                  className="absolute top-1/2 h-5 w-5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white bg-transparent shadow-[0_0_0_1px_rgba(17,24,39,0.32)]"
                  style={{ left: `${(hsv.h / 360) * 100}%` }}
                />
              </div>
            </div>
          </div>

          <div className="space-y-2">
            <div className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
              {t("settings.themeQuickPicks")}
            </div>
            <div className="grid grid-cols-8 gap-2">
              {quickColors.map((color) => (
                <button
                  key={`${label}-${color}`}
                  type="button"
                  className={cn(
                    "h-8 w-8 rounded-lg border shadow-sm transition-transform hover:scale-105",
                    value.toLowerCase() === color.toLowerCase()
                      ? "border-primary ring-2 ring-primary/25"
                      : "border-border/70",
                  )}
                  style={{ backgroundColor: color }}
                  onClick={() => onChange(color)}
                />
              ))}
            </div>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}

function ThemePreviewPanel({
  mode,
  palette,
  t,
}: {
  mode: EditablePaletteMode;
  palette: CustomThemePalette;
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  const statSurface = mixHex(palette.card, palette.background, 0.22);

  return (
    <div className="space-y-3 rounded-2xl border border-border/70 bg-card/55 p-4 shadow-sm">
      <div className="space-y-1">
        <h4 className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
          {t("settings.themePreview")}
        </h4>
        <p className="text-xs text-muted-foreground">
          {t("settings.themePreviewHint")}
        </p>
      </div>

      <div
        className="overflow-hidden rounded-[28px] border shadow-xl"
        style={{
          backgroundColor: palette.background,
          borderColor: palette.border,
          color: palette.foreground,
        }}
      >
        <div
          className="flex items-center justify-between border-b px-4 py-3"
          style={{ borderColor: palette.border }}
        >
          <div>
            <div className="text-sm font-semibold">CC Switch</div>
            <div className="text-xs" style={{ color: palette.mutedForeground }}>
              {mode === "light"
                ? t("settings.themePaletteLight")
                : t("settings.themePaletteDark")}
            </div>
          </div>
          <button
            type="button"
            className="rounded-full px-3 py-1.5 text-xs font-medium shadow-sm"
            style={{
              backgroundColor: palette.primary,
              color: palette.primaryForeground,
            }}
          >
            {t("common.save")}
          </button>
        </div>

        <div className="grid gap-3 p-4">
          <div
            className="rounded-3xl border p-4"
            style={{
              backgroundColor: palette.card,
              borderColor: palette.border,
              color: palette.cardForeground,
            }}
          >
            <div className="mb-3 flex items-start justify-between gap-3">
              <div>
                <div className="text-sm font-semibold">
                  {t("settings.themePreviewCardTitle")}
                </div>
                <div
                  className="text-xs"
                  style={{ color: palette.mutedForeground }}
                >
                  {t("settings.themePreviewCardDescription")}
                </div>
              </div>
              <span
                className="rounded-full px-2.5 py-1 text-[10px] font-semibold"
                style={{
                  backgroundColor: palette.accent,
                  color: palette.accentForeground,
                }}
              >
                {t("common.enabled")}
              </span>
            </div>

            <div className="grid gap-2 sm:grid-cols-2">
              <div
                className="rounded-2xl border p-3"
                style={{
                  backgroundColor: statSurface,
                  borderColor: palette.border,
                }}
              >
                <div
                  className="text-[11px] uppercase tracking-wide"
                  style={{ color: palette.mutedForeground }}
                >
                  {t("settings.themeColorPrimary")}
                </div>
                <div className="mt-1 text-sm font-semibold">
                  {t("settings.themePreviewStatA")}
                </div>
              </div>
              <div
                className="rounded-2xl border p-3"
                style={{
                  backgroundColor: statSurface,
                  borderColor: palette.border,
                }}
              >
                <div
                  className="text-[11px] uppercase tracking-wide"
                  style={{ color: palette.mutedForeground }}
                >
                  {t("settings.themeColorAccent")}
                </div>
                <div className="mt-1 text-sm font-semibold">
                  {t("settings.themePreviewStatB")}
                </div>
              </div>
            </div>

            <div className="mt-4 space-y-3">
              <div
                className="rounded-2xl border p-3"
                style={{
                  backgroundColor: palette.popover,
                  borderColor: palette.input,
                  color: palette.popoverForeground,
                }}
              >
                <div className="mb-2 text-xs font-medium">
                  {t("settings.themeSectionPopover")}
                </div>
                <div
                  className="rounded-xl border px-3 py-2 text-sm"
                  style={{
                    borderColor: palette.input,
                    color: palette.mutedForeground,
                  }}
                >
                  sk-cc-switch-demo
                </div>
              </div>

              <div className="flex flex-wrap gap-2">
                <button
                  type="button"
                  className="rounded-xl px-3 py-2 text-xs font-medium shadow-sm"
                  style={{
                    backgroundColor: palette.primary,
                    color: palette.primaryForeground,
                  }}
                >
                  {t("common.confirm")}
                </button>
                <button
                  type="button"
                  className="rounded-xl border px-3 py-2 text-xs font-medium"
                  style={{
                    backgroundColor: palette.secondary,
                    borderColor: palette.border,
                    color: palette.secondaryForeground,
                  }}
                >
                  {t("common.cancel")}
                </button>
                <button
                  type="button"
                  className="rounded-xl border px-3 py-2 text-xs font-medium"
                  style={{
                    backgroundColor: palette.accent,
                    borderColor: palette.border,
                    color: palette.accentForeground,
                  }}
                >
                  {t("settings.themePreviewAccent")}
                </button>
                <button
                  type="button"
                  className="rounded-xl px-3 py-2 text-xs font-medium"
                  style={{
                    backgroundColor: palette.destructive,
                    color: palette.destructiveForeground,
                  }}
                >
                  {t("settings.themeSectionDestructive")}
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
