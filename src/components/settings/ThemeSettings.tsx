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
import {
  type CustomThemePalette,
  getReadableTextColor,
  hexToHsv,
  hexToRgb,
  hsvToHex,
  isHexColor,
  mixHex,
  normalizeHex,
} from "@/lib/theme/customTheme";
import { cn } from "@/lib/utils";

const CUSTOM_THEME_FIELDS = [
  { key: "background", labelKey: "settings.themeColorBackground" },
  { key: "foreground", labelKey: "settings.themeColorForeground" },
  { key: "primary", labelKey: "settings.themeColorPrimary" },
  {
    key: "primaryForeground",
    labelKey: "settings.themeColorPrimaryForeground",
  },
  { key: "secondary", labelKey: "settings.themeColorSecondary" },
  {
    key: "secondaryForeground",
    labelKey: "settings.themeColorSecondaryForeground",
  },
  { key: "accent", labelKey: "settings.themeColorAccent" },
  { key: "accentForeground", labelKey: "settings.themeColorAccentForeground" },
  { key: "card", labelKey: "settings.themeColorCard" },
  { key: "cardForeground", labelKey: "settings.themeColorCardForeground" },
  { key: "popover", labelKey: "settings.themeColorPopover" },
  {
    key: "popoverForeground",
    labelKey: "settings.themeColorPopoverForeground",
  },
  { key: "muted", labelKey: "settings.themeColorMuted" },
  { key: "mutedForeground", labelKey: "settings.themeColorMutedForeground" },
  { key: "destructive", labelKey: "settings.themeColorDestructive" },
  {
    key: "destructiveForeground",
    labelKey: "settings.themeColorDestructiveForeground",
  },
  { key: "success", labelKey: "settings.themeColorSuccess" },
  { key: "info", labelKey: "settings.themeColorInfo" },
  { key: "warning", labelKey: "settings.themeColorWarning" },
  { key: "error", labelKey: "settings.themeColorError" },
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
  {
    id: "semantic",
    titleKey: "settings.themeSectionSemanticAdvanced",
    rowKeys: ["success", "info", "warning", "error"] as const,
    allowSync: false,
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
const PRESET_SWATCHES = {
  default: ["#ffffff", "#1f9cff", "#f4f4f5"],
  bubblegum: ["#fff1f7", "#f25ca8", "#ffd6a3"],
} as const;

type EditablePaletteMode = "light" | "dark";
type CustomThemeFieldKey = (typeof CUSTOM_THEME_FIELDS)[number]["key"];
type ThemeSectionId = (typeof SECTION_DEFINITIONS)[number]["id"];

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
      return getReadableTextColor(palette.background);
    case "primaryForeground":
      return getReadableTextColor(palette.primary);
    case "secondaryForeground":
      return getReadableTextColor(palette.secondary);
    case "accentForeground":
      return getReadableTextColor(palette.accent);
    case "cardForeground":
      return getReadableTextColor(palette.card);
    case "popoverForeground":
      return getReadableTextColor(palette.popover);
    case "mutedForeground":
      return getReadableTextColor(palette.muted);
    case "destructiveForeground":
      return getReadableTextColor(palette.destructive);
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
      return { primaryForeground: getReadableTextColor(palette.primary) };
    case "secondary":
      return { secondaryForeground: getReadableTextColor(palette.secondary) };
    case "accent":
      return { accentForeground: getReadableTextColor(palette.accent) };
    case "base":
      return { foreground: getReadableTextColor(palette.background) };
    case "card":
      return { cardForeground: getReadableTextColor(palette.card) };
    case "popover":
      return { popoverForeground: getReadableTextColor(palette.popover) };
    case "muted":
      return { mutedForeground: getReadableTextColor(palette.muted) };
    case "destructive":
      return {
        destructiveForeground: getReadableTextColor(palette.destructive),
      };
    case "semantic":
      return {};
    case "borderInput":
      return {
        input: palette.border,
        ring: palette.primary,
      };
    default:
      return {};
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
    semantic: false,
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
      return {
        ...section,
        allowSync: "allowSync" in section ? section.allowSync : true,
        rows,
      };
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
    resolvedPaletteMode === "dark" ? customTheme.dark : currentPalette;

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
              swatches={[...PRESET_SWATCHES.default]}
              useInlineSwatches
              onClick={() => setThemePreset("default")}
            />
            <PresetCard
              active={themePreset === "bubblegum"}
              name={t("settings.themePresetBubblegum")}
              description={t("settings.themePresetBubblegumDescription")}
              swatches={[...PRESET_SWATCHES.bubblegum]}
              useInlineSwatches
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
                                const canSync = section.allowSync !== false;
                                const previewColors = section.rows
                                  .map((fieldKey) => currentPalette[fieldKey])
                                  .slice(0, section.id === "semantic" ? 4 : 3);
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
                                        {canSync ? (
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
                                        ) : (
                                          <div className="h-9 w-9 shrink-0" />
                                        )}
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
          } else if (event.key === "Escape") {
            event.preventDefault();
            setDraftValue(value);
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
  const dragCleanupRef = useRef<(() => void) | null>(null);
  const [draftValue, setDraftValue] = useState(value);
  const hsv = useMemo(() => hexToHsv(value), [value]);
  const hueColor = useMemo(() => hsvToHex(hsv.h, 1, 1), [hsv.h]);

  useEffect(() => {
    setDraftValue(value);
  }, [value]);

  useEffect(() => {
    return () => {
      dragCleanupRef.current?.();
      dragCleanupRef.current = null;
    };
  }, []);

  const clearDragListeners = () => {
    dragCleanupRef.current?.();
    dragCleanupRef.current = null;
  };

  const commitDraft = (nextValue = draftValue) => {
    const normalized = normalizeHex(nextValue);
    if (!isHexColor(normalized)) {
      setDraftValue(value);
      return;
    }

    setDraftValue(normalized);
    onChange(normalized);
  };

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
    clearDragListeners();
    updater(event.clientX, event.clientY);

    const handleMove = (moveEvent: PointerEvent) =>
      updater(moveEvent.clientX, moveEvent.clientY);
    const handleUp = () => {
      clearDragListeners();
    };

    window.addEventListener("pointermove", handleMove);
    window.addEventListener("pointerup", handleUp);
    window.addEventListener("pointercancel", handleUp);
    dragCleanupRef.current = () => {
      window.removeEventListener("pointermove", handleMove);
      window.removeEventListener("pointerup", handleUp);
      window.removeEventListener("pointercancel", handleUp);
    };
  };

  const handleSquareKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const step = event.shiftKey ? 0.1 : 0.05;
    let nextS = hsv.s;
    let nextV = hsv.v;

    switch (event.key) {
      case "ArrowLeft":
        nextS = Math.max(0, hsv.s - step);
        break;
      case "ArrowRight":
        nextS = Math.min(1, hsv.s + step);
        break;
      case "ArrowUp":
        nextV = Math.min(1, hsv.v + step);
        break;
      case "ArrowDown":
        nextV = Math.max(0, hsv.v - step);
        break;
      default:
        return;
    }

    event.preventDefault();
    onChange(hsvToHex(hsv.h, nextS, nextV));
  };

  const handleHueKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const step = event.shiftKey ? 15 : 5;
    let nextHue = hsv.h;

    switch (event.key) {
      case "ArrowLeft":
      case "ArrowDown":
        nextHue = (hsv.h - step + 360) % 360;
        break;
      case "ArrowRight":
      case "ArrowUp":
        nextHue = (hsv.h + step) % 360;
        break;
      default:
        return;
    }

    event.preventDefault();
    onChange(hsvToHex(nextHue, hsv.s, hsv.v));
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
      <PopoverContent className="w-[19rem] rounded-2xl border-border/70 p-3">
        <div className="space-y-3">
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
              className="h-10 w-10 rounded-xl border border-border/70 shadow-sm"
              style={{ backgroundColor: value }}
            />
          </div>

          <label className="block space-y-1.5">
            <span className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
              {t("settings.themeColorHex")}
            </span>
            <Input
              value={draftValue}
              onChange={(event) => setDraftValue(event.target.value)}
              onBlur={() => commitDraft()}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  commitDraft();
                } else if (event.key === "Escape") {
                  event.preventDefault();
                  setDraftValue(value);
                }
              }}
              aria-label={`${label} ${t("settings.themeColorHex")}`}
              className="h-9 rounded-xl font-mono"
            />
          </label>

          <div className="space-y-2.5">
            <div className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
              {t("settings.themeColorField")}
            </div>
            <div
              ref={squareRef}
              role="slider"
              tabIndex={0}
              aria-label={`${label} ${t("settings.themeColorField")}`}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={Math.round(hsv.s * 100)}
              className="relative h-36 w-full cursor-crosshair overflow-hidden rounded-xl border border-border/70"
              style={{ backgroundColor: hueColor }}
              onPointerDown={(event) => startDrag(event, updateFromSquare)}
              onKeyDown={handleSquareKeyDown}
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

            <div className="space-y-1.5">
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
                role="slider"
                tabIndex={0}
                aria-label={`${label} ${t("settings.themeColorHue")}`}
                aria-valuemin={0}
                aria-valuemax={360}
                aria-valuenow={Math.round(hsv.h)}
                className="relative h-3.5 w-full cursor-ew-resize rounded-full border border-border/70"
                style={{
                  background:
                    "linear-gradient(90deg, #ff0000 0%, #ffff00 16.6%, #00ff00 33.3%, #00ffff 50%, #0000ff 66.6%, #ff00ff 83.3%, #ff0000 100%)",
                }}
                onPointerDown={(event) =>
                  startDrag(event, (clientX) => updateFromHue(clientX))
                }
                onKeyDown={handleHueKeyDown}
              >
                <div
                  className="absolute top-1/2 h-5 w-5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white bg-transparent shadow-[0_0_0_1px_rgba(17,24,39,0.32)]"
                  style={{ left: `${(hsv.h / 360) * 100}%` }}
                />
              </div>
            </div>
          </div>

          <div className="space-y-1.5">
            <div className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
              {t("settings.themeQuickPicks")}
            </div>
            <div className="grid grid-cols-8 gap-1.5">
              {quickColors.map((color) => (
                <button
                  key={`${label}-${color}`}
                  type="button"
                  aria-label={`${label} ${color.toUpperCase()}`}
                  className={cn(
                    "h-7 w-7 rounded-md border shadow-sm transition-transform hover:scale-105",
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
  const ringShadow = (() => {
    const { r, g, b } = hexToRgb(palette.ring);
    return `0 0 0 3px rgba(${r}, ${g}, ${b}, 0.18)`;
  })();
  const previewButtonClass =
    "inline-flex items-center justify-center rounded-xl px-3 py-2 text-xs font-medium";
  const previewInputClass =
    "flex h-10 items-center rounded-xl border bg-transparent px-3 text-sm shadow-none";

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
          <div
            className="inline-flex items-center justify-center rounded-full px-3 py-1.5 text-xs font-medium shadow-sm"
            style={{
              backgroundColor: palette.primary,
              color: palette.primaryForeground,
            }}
          >
            {t("common.save")}
          </div>
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

            <div className="mt-4 space-y-3">
              <div className="grid gap-3 xl:grid-cols-[minmax(0,0.78fr)_minmax(0,1.22fr)]">
                <div
                  className="rounded-2xl border p-3"
                  style={{
                    backgroundColor: statSurface,
                    borderColor: palette.border,
                  }}
                >
                  <div
                    className="mb-2 text-[11px] font-medium uppercase tracking-wide"
                    style={{ color: palette.mutedForeground }}
                  >
                    {t("settings.themePreviewStatusLabel")}
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <span
                      className="rounded-full px-2.5 py-1 text-[10px] font-semibold"
                      style={{
                        backgroundColor: palette.accent,
                        color: palette.accentForeground,
                      }}
                    >
                      {t("common.enabled")}
                    </span>
                    <span
                      className="rounded-full px-2.5 py-1 text-[10px] font-semibold"
                      style={{
                        backgroundColor: palette.secondary,
                        color: palette.secondaryForeground,
                        border: `1px solid ${palette.border}`,
                      }}
                    >
                      {t("settings.themePreviewSelected")}
                    </span>
                  </div>
                </div>

                <div
                  className="rounded-2xl border p-3"
                  style={{
                    backgroundColor: palette.popover,
                    borderColor: palette.input,
                    color: palette.popoverForeground,
                  }}
                >
                  <div
                    className="mb-3 text-[11px] font-medium uppercase tracking-wide"
                    style={{ color: palette.popoverForeground }}
                  >
                    {t("settings.themePreviewInputLabel")}
                  </div>

                  <div className="grid gap-2.5 sm:grid-cols-2">
                    <div className="space-y-1.5">
                      <div
                        className="text-[11px] font-medium uppercase tracking-wide"
                        style={{ color: palette.mutedForeground }}
                      >
                        {t("settings.themePreviewInputDefault")}
                      </div>
                      <div
                        className={previewInputClass}
                        style={{
                          backgroundColor: palette.background,
                          borderColor: palette.input,
                          color: palette.mutedForeground,
                        }}
                      >
                        {t("settings.themePreviewInputPlaceholder")}
                      </div>
                    </div>

                    <div className="space-y-1.5">
                      <div
                        className="text-[11px] font-medium uppercase tracking-wide"
                        style={{ color: palette.mutedForeground }}
                      >
                        {t("settings.themePreviewInputFocused")}
                      </div>
                      <div
                        className="rounded-xl"
                        style={{ boxShadow: ringShadow }}
                      >
                        <div
                          className={previewInputClass}
                          style={{
                            backgroundColor: palette.background,
                            borderColor: palette.ring,
                            color: palette.popoverForeground,
                          }}
                        >
                          sk-cc-switch-demo
                        </div>
                      </div>
                    </div>
                  </div>
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
                  className="mb-2 text-[11px] font-medium uppercase tracking-wide"
                  style={{ color: palette.mutedForeground }}
                >
                  {t("settings.themePreviewActionsLabel")}
                </div>
                <div
                  className="mb-3 text-xs"
                  style={{ color: palette.mutedForeground }}
                >
                  {t("settings.themePreviewActionsHint")}
                </div>

                <div className="flex flex-wrap gap-2">
                  <div
                    className={`${previewButtonClass} shadow-sm`}
                    style={{
                      backgroundColor: palette.primary,
                      color: palette.primaryForeground,
                    }}
                  >
                    {t("common.confirm")}
                  </div>
                  <div
                    className={`${previewButtonClass} border`}
                    style={{
                      backgroundColor: palette.secondary,
                      borderColor: palette.border,
                      color: palette.secondaryForeground,
                    }}
                  >
                    {t("common.cancel")}
                  </div>
                  <div
                    className={`${previewButtonClass} border`}
                    style={{
                      backgroundColor: palette.accent,
                      borderColor: palette.border,
                      color: palette.accentForeground,
                    }}
                  >
                    {t("settings.themePreviewAccent")}
                  </div>
                  <div
                    className={previewButtonClass}
                    style={{
                      backgroundColor: palette.destructive,
                      color: palette.destructiveForeground,
                    }}
                  >
                    {t("settings.themeSectionDestructive")}
                  </div>
                </div>
              </div>

              <div
                className="rounded-2xl border p-3"
                style={{
                  backgroundColor: palette.card,
                  borderColor: palette.border,
                }}
              >
                <div
                  className="mb-2 text-[11px] font-medium uppercase tracking-wide"
                  style={{ color: palette.mutedForeground }}
                >
                  {t("settings.themePreviewToastLabel")}
                </div>
                <div
                  className="mb-3 text-xs"
                  style={{ color: palette.mutedForeground }}
                >
                  {t("settings.themePreviewToastHint")}
                </div>

                <div className="grid gap-2 sm:grid-cols-2">
                  {[
                    {
                      key: "success",
                      title: t("settings.themePreviewToastSuccess"),
                      background: mixHex(palette.success, palette.card, 0.88),
                      border: mixHex(palette.success, palette.card, 0.74),
                      color: palette.success,
                    },
                    {
                      key: "info",
                      title: t("settings.themePreviewToastInfo"),
                      background: mixHex(palette.info, palette.card, 0.88),
                      border: mixHex(palette.info, palette.card, 0.74),
                      color: palette.info,
                    },
                    {
                      key: "warning",
                      title: t("settings.themePreviewToastWarning"),
                      background: mixHex(palette.warning, palette.card, 0.88),
                      border: mixHex(palette.warning, palette.card, 0.74),
                      color: palette.warning,
                    },
                    {
                      key: "error",
                      title: t("settings.themePreviewToastError"),
                      background: mixHex(palette.error, palette.card, 0.88),
                      border: mixHex(palette.error, palette.card, 0.74),
                      color: palette.error,
                    },
                  ].map((toastPreview) => (
                    <div
                      key={toastPreview.key}
                      className="rounded-xl border px-3 py-2.5 shadow-sm"
                      style={{
                        backgroundColor: toastPreview.background,
                        borderColor: toastPreview.border,
                        color: toastPreview.color,
                      }}
                    >
                      <div className="text-xs font-semibold">
                        {toastPreview.title}
                      </div>
                      <div className="mt-1 text-[11px] opacity-90">
                        {t("settings.themePreviewToastBody")}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
