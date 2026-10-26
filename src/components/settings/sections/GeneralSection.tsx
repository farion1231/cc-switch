import { useTranslation } from "react-i18next";
import { ChevronRight, Monitor, Moon, Sun } from "lucide-react";
import type { SettingsFormState } from "@/hooks/useSettings";
import { useTheme } from "@/components/theme-provider";
import { SegmentedControl } from "@/components/ui/segmented-control";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { isLinux, isMac } from "@/lib/platform";
import {
  setDensity,
  setZoom,
  useInterfacePrefs,
  ZOOM_LEVELS,
  type Density,
  type ZoomLevel,
} from "@/lib/interfacePrefs";
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
  { value: "tr", labelKey: "settings.languageOptionTurkish" },
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
  const { density, zoom } = useInterfacePrefs();

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
                  className="w-[160px]"
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
          <SettingsRow
            label={t("settings.general.quotaDisplay")}
            help={{
              title: t("settings.general.quotaDisplay"),
              body: t("settings.general.quotaDisplayHelp"),
            }}
            control={
              <SegmentedControl
                size="sm"
                aria-label={t("settings.general.quotaDisplay")}
                value={settings.quotaDisplay ?? "left"}
                onValueChange={(value) =>
                  void onAutoSave({ quotaDisplay: value })
                }
                items={[
                  {
                    value: "left",
                    label: t("settings.general.quotaDisplayLeft"),
                  },
                  {
                    value: "used",
                    label: t("settings.general.quotaDisplayUsed"),
                  },
                ]}
              />
            }
          />
          <SettingsRow
            label={t("settings.general.density")}
            help={{
              title: t("settings.general.density"),
              body: t("settings.general.densityHelp"),
            }}
            control={
              <SegmentedControl<Density>
                size="sm"
                aria-label={t("settings.general.density")}
                value={density}
                onValueChange={setDensity}
                items={[
                  {
                    value: "comfortable",
                    label: t("settings.general.densityComfortable"),
                  },
                  {
                    value: "compact",
                    label: t("settings.general.densityCompact"),
                  },
                ]}
              />
            }
          />
          <SettingsRow
            label={t("settings.general.zoom")}
            help={{
              title: t("settings.general.zoom"),
              body: t("settings.general.zoomHelp", {
                shortcut: isMac() ? "⌘" : "Ctrl",
              }),
            }}
            control={
              <SegmentedControl<`${ZoomLevel}`>
                size="sm"
                aria-label={t("settings.general.zoom")}
                value={`${zoom}`}
                onValueChange={(value) => setZoom(Number(value) as ZoomLevel)}
                items={ZOOM_LEVELS.map((level) => ({
                  value: `${level}` as const,
                  label: `${Math.round(level * 100)}%`,
                }))}
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
              summary={t("settings.silentStartupDescription")}
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
          {/* Linux（AppIndicator）只能出原生菜单，没得选 */}
          {!isLinux() && (
            <SettingsRow
              label={t("settings.general.trayStyle")}
              help={{
                title: t("settings.general.trayStyle"),
                body: t("settings.general.trayStyleHelp"),
              }}
              control={
                <SegmentedControl
                  size="sm"
                  aria-label={t("settings.general.trayStyle")}
                  value={settings.nativeTrayMenu ? "menu" : "panel"}
                  onValueChange={(value) =>
                    void onAutoSave({ nativeTrayMenu: value === "menu" })
                  }
                  items={[
                    {
                      value: "panel",
                      label: t("settings.general.trayStylePanel"),
                    },
                    {
                      value: "menu",
                      label: t("settings.general.trayStyleMenu"),
                    },
                  ]}
                />
              }
            />
          )}
          {isLinux() && (
            <SettingsSwitchRow
              label={t("settings.useAppWindowControls")}
              summary={t("settings.useAppWindowControlsSummary")}
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
