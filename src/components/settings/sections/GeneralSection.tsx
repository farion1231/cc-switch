import { useTranslation } from "react-i18next";
import {
  ChevronRight,
  Minus,
  Monitor,
  Moon,
  Plus,
  RotateCcw,
  Sun,
} from "lucide-react";
import type { SettingsFormState } from "@/hooks/useSettings";
import { useTheme } from "@/components/theme-provider";
import {
  FONT_FAMILY_OPTIONS,
  FONT_SIZE_OPTIONS,
  useAppearance,
} from "@/components/appearance-provider";
import { SegmentedControl } from "@/components/ui/segmented-control";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { isLinux } from "@/lib/platform";
import { TerminalSelect } from "@/components/settings/TerminalSettings";
import {
  SettingsBlock,
  SettingsCard,
  SettingsRow,
  SettingsSwitchRow,
} from "@/components/settings/SettingsLayout";

type Language = SettingsFormState["language"];

const LANGUAGES: { value: Language; labelKey: string }[] = [
  { value: "zh", labelKey: "settings.languageOptionChinese" },
  { value: "zh-TW", labelKey: "settings.languageOptionTraditionalChinese" },
  { value: "en", labelKey: "settings.languageOptionEnglish" },
  { value: "ja", labelKey: "settings.languageOptionJapanese" },
];

interface GeneralSectionProps {
  settings: SettingsFormState;
  onAutoSave: (updates: Partial<SettingsFormState>) => Promise<boolean>;
  onOpenApps: () => void;
}

export function GeneralSection({
  settings,
  onAutoSave,
  onOpenApps,
}: GeneralSectionProps) {
  const { t } = useTranslation();
  const { theme, setTheme } = useTheme();
  const {
    fontFamily,
    fontSize,
    pageZoom,
    setFontFamily,
    setFontSize,
    setPageZoom,
    resetPageZoom,
  } = useAppearance();

  return (
    <>
      <SettingsBlock title={t("settings.general.appearance")}>
        <SettingsCard>
          <SettingsRow
            label={t("settings.language")}
            control={
              <Select
                value={settings.language}
                onValueChange={(value) =>
                  void onAutoSave({ language: value as Language })
                }
              >
                <SelectTrigger
                  className="h-8 w-[160px]"
                  aria-label={t("settings.language")}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {LANGUAGES.map((language) => (
                    <SelectItem key={language.value} value={language.value}>
                      {t(language.labelKey)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            }
          />
          <SettingsRow
            label={t("settings.fontFamily")}
            control={
              <Select
                value={fontFamily}
                onValueChange={(value) =>
                  setFontFamily(value as typeof fontFamily)
                }
              >
                <SelectTrigger
                  className="h-8 w-[180px]"
                  aria-label={t("settings.fontFamily")}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {FONT_FAMILY_OPTIONS.map((font) => (
                    <SelectItem
                      key={font.value}
                      value={font.value}
                      style={{ fontFamily: font.css }}
                    >
                      {t(font.labelKey)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            }
          />
          <SettingsRow
            label={t("settings.fontSize")}
            control={
              <Select
                value={String(fontSize)}
                onValueChange={(value) => setFontSize(Number(value))}
              >
                <SelectTrigger
                  className="h-8 w-[120px]"
                  aria-label={t("settings.fontSize")}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {FONT_SIZE_OPTIONS.map((size) => (
                    <SelectItem key={size} value={String(size)}>
                      {size}px
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            }
          />
          <SettingsRow
            label={t("settings.pageZoom")}
            help={{
              title: t("settings.pageZoom"),
              body: t("settings.pageZoomHint"),
            }}
            control={
              <div className="flex items-center gap-1">
                <Button
                  variant="quiet"
                  size="icon-compact"
                  aria-label={t("settings.zoomOut")}
                  onClick={() => setPageZoom(pageZoom - 0.2)}
                  disabled={pageZoom <= 0.5}
                >
                  <Minus className="h-4 w-4" />
                </Button>
                <button
                  type="button"
                  className="min-w-[56px] rounded-control px-2 py-1 text-body font-medium text-fg-1 hover:bg-subtle"
                  onClick={resetPageZoom}
                  aria-label={t("settings.resetZoom")}
                >
                  {Math.round(pageZoom * 100)}%
                </button>
                <Button
                  variant="quiet"
                  size="icon-compact"
                  aria-label={t("settings.zoomIn")}
                  onClick={() => setPageZoom(pageZoom + 0.2)}
                  disabled={pageZoom >= 2}
                >
                  <Plus className="h-4 w-4" />
                </Button>
                <Button
                  variant="quiet"
                  size="icon-compact"
                  aria-label={t("settings.resetZoom")}
                  onClick={resetPageZoom}
                >
                  <RotateCcw className="h-3.5 w-3.5" />
                </Button>
              </div>
            }
          />
          <SettingsRow
            label={t("settings.theme")}
            control={
              <SegmentedControl
                size="sm"
                aria-label={t("settings.theme")}
                value={theme}
                onValueChange={setTheme}
                items={[
                  {
                    value: "system",
                    label: t("settings.themeSystem"),
                    icon: Monitor,
                  },
                  {
                    value: "light",
                    label: t("settings.themeLight"),
                    icon: Sun,
                  },
                  {
                    value: "dark",
                    label: t("settings.themeDark"),
                    icon: Moon,
                  },
                ]}
              />
            }
          />
        </SettingsCard>
      </SettingsBlock>

      <SettingsBlock title={t("settings.general.sidebarAndHeader")}>
        <SettingsCard>
          <SettingsRow
            label={t("settings.general.visibleApps")}
            control={
              <Button variant="quiet" size="compact" onClick={onOpenApps}>
                {t("settings.general.goToApps")}
                <ChevronRight className="h-3.5 w-3.5" />
              </Button>
            }
          />
          <SettingsSwitchRow
            label={t("settings.appVisibility.showProfileSwitcher")}
            help={{
              title: t("settings.appVisibility.showProfileSwitcher"),
              body: t("settings.appVisibility.showProfileSwitcherDescription"),
            }}
            checked={settings.showProfileSwitcher ?? false}
            onCheckedChange={(value) =>
              void onAutoSave({ showProfileSwitcher: value })
            }
          />
        </SettingsCard>
      </SettingsBlock>

      <SettingsBlock title={t("settings.general.updates")}>
        <SettingsCard>
          <SettingsSwitchRow
            label={t("settings.general.checkToolUpdates")}
            help={{
              title: t("settings.general.checkToolUpdates"),
              body: t("settings.general.checkToolUpdatesHelp"),
            }}
            checked={settings.checkToolUpdatesOnStartup ?? false}
            onCheckedChange={(value) =>
              void onAutoSave({ checkToolUpdatesOnStartup: value })
            }
          />
        </SettingsCard>
      </SettingsBlock>

      <SettingsBlock title={t("settings.general.windowAndTerminal")}>
        <SettingsCard>
          <SettingsSwitchRow
            label={t("settings.launchOnStartup")}
            checked={!!settings.launchOnStartup}
            onCheckedChange={(value) =>
              void onAutoSave({ launchOnStartup: value })
            }
          />
          {settings.launchOnStartup && (
            <SettingsSwitchRow
              label={t("settings.silentStartup")}
              help={{
                title: t("settings.silentStartup"),
                body: t("settings.silentStartupDescription"),
              }}
              checked={!!settings.silentStartup}
              onCheckedChange={(value) =>
                void onAutoSave({ silentStartup: value })
              }
            />
          )}
          <SettingsSwitchRow
            label={t("settings.minimizeToTray")}
            help={{
              title: t("settings.minimizeToTray"),
              body: t("settings.general.minimizeToTrayHelp"),
            }}
            checked={settings.minimizeToTrayOnClose}
            onCheckedChange={(value) =>
              void onAutoSave({ minimizeToTrayOnClose: value })
            }
          />
          {isLinux() && (
            <SettingsSwitchRow
              label={t("settings.useAppWindowControls")}
              help={{
                title: t("settings.useAppWindowControls"),
                body: t("settings.useAppWindowControlsDescription"),
              }}
              checked={!!settings.useAppWindowControls}
              onCheckedChange={(value) =>
                void onAutoSave({ useAppWindowControls: value })
              }
            />
          )}
          <SettingsRow
            label={t("settings.terminal.title")}
            help={{
              title: t("settings.terminal.title"),
              body: t("settings.general.terminalHelp"),
            }}
            control={
              <TerminalSelect
                className="h-8 w-[160px]"
                value={settings.preferredTerminal}
                onChange={(terminal) =>
                  void onAutoSave({ preferredTerminal: terminal })
                }
              />
            }
          />
        </SettingsCard>
      </SettingsBlock>
    </>
  );
}
