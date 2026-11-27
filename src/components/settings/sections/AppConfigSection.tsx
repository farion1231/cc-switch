import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import type {
  ResolvedDirectories,
  SettingsFormState,
} from "@/hooks/useSettings";
import type { Settings } from "@/types";
import { Button } from "@/components/ui/button";
import { DirectoryInput } from "@/components/settings/DirectorySettings";
import {
  DIRECTORY_APP,
  DIRECTORY_APPS,
  type DirectoryAppId,
  sanitizeDir,
} from "@/config/directoryApps";
import { CodexAuthSettings } from "@/components/settings/CodexAuthSettings";
import { CopilotVscodeSettings } from "@/components/settings/CopilotVscodeSettings";
import { APP_DISPLAY_NAME, AppGlyph } from "@/components/shell/AppGlyph";
import {
  SettingsBlock,
  SettingsCard,
  SettingsRow,
  SettingsSwitchRow,
} from "@/components/settings/SettingsLayout";

interface AppConfigSectionProps {
  settings: SettingsFormState;
  savedSettings?: Settings;
  resolvedDirs: ResolvedDirectories;
  isSaving: boolean;
  onAutoSave: (updates: Partial<SettingsFormState>) => Promise<boolean>;
  onDirectoryChange: (app: DirectoryAppId, value?: string) => void;
  onBrowseDirectory: (app: DirectoryAppId) => Promise<void>;
  onResetDirectory: (app: DirectoryAppId) => Promise<void>;
  onSaveDirectories: () => Promise<void>;
}

/**
 * 设置 → 应用配置：每个应用一节，放只属于它的开关和配置目录。
 * 目录改动要点「保存」才写入（各节自己的保存按钮，保存时一并写入所有改过的目录）。
 */
export function AppConfigSection({
  settings,
  savedSettings,
  resolvedDirs,
  isSaving,
  onAutoSave,
  onDirectoryChange,
  onBrowseDirectory,
  onResetDirectory,
  onSaveDirectories,
}: AppConfigSectionProps) {
  const { t } = useTranslation();

  const isDirty = (app: DirectoryAppId) => {
    const { field } = DIRECTORY_APP[app];
    return sanitizeDir(settings[field]) !== sanitizeDir(savedSettings?.[field]);
  };

  const directoryRow = (app: DirectoryAppId) => (
    <SettingsRow
      label={t("settings.appConfig.configDir")}
      help={{
        title: t("settings.appConfig.configDir"),
        body:
          app === "agy"
            ? t("settings.agyConfigDirDescription")
            : t("settings.configDirectoryDescription"),
      }}
    >
      <DirectoryInput
        label=""
        value={settings[DIRECTORY_APP[app].field]}
        resolvedValue={resolvedDirs[app]}
        placeholder={t(DIRECTORY_APP[app].placeholderKey)}
        onChange={(value) => onDirectoryChange(app, value)}
        onBrowse={() => onBrowseDirectory(app)}
        onReset={() => onResetDirectory(app)}
      />
    </SettingsRow>
  );

  const saveButton = (app: DirectoryAppId) =>
    isDirty(app) ? (
      <Button
        variant="solid"
        size="compact"
        disabled={isSaving}
        onClick={() => void onSaveDirectories()}
      >
        {isSaving && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
        {t("common.save")}
      </Button>
    ) : null;

  const blockTitle = (app: DirectoryAppId) => (
    <span className="flex items-center gap-2">
      <AppGlyph app={app} size={16} badgeClassName="bg-app" />
      {APP_DISPLAY_NAME[app]}
    </span>
  );

  return (
    <>
      {DIRECTORY_APPS.map(({ app }) => (
        <SettingsBlock
          key={app}
          id={`app-config-${app}`}
          className="rounded-panel"
          title={blockTitle(app)}
          actions={saveButton(app)}
        >
          <SettingsCard>
            {app === "claude" && (
              <>
                <SettingsSwitchRow
                  label={t("settings.enableClaudePluginIntegration")}
                  help={{
                    title: t("settings.enableClaudePluginIntegration"),
                    body: t(
                      "settings.enableClaudePluginIntegrationDescription",
                    ),
                  }}
                  checked={!!settings.enableClaudePluginIntegration}
                  onCheckedChange={(value) =>
                    void onAutoSave({ enableClaudePluginIntegration: value })
                  }
                />
                <SettingsSwitchRow
                  label={t("settings.skipClaudeOnboarding")}
                  help={{
                    title: t("settings.skipClaudeOnboarding"),
                    body: t("settings.skipClaudeOnboardingDescription"),
                  }}
                  checked={!!settings.skipClaudeOnboarding}
                  onCheckedChange={(value) =>
                    void onAutoSave({ skipClaudeOnboarding: value })
                  }
                />
              </>
            )}
            {app === "codex" && (
              <CodexAuthSettings
                settings={settings}
                onChange={onAutoSave}
                codexConfigDir={savedSettings?.codexConfigDir}
              />
            )}
            {directoryRow(app)}
          </SettingsCard>
        </SettingsBlock>
      ))}
      <CopilotVscodeSettings
        settings={settings}
        isSaving={isSaving}
        onAutoSave={onAutoSave}
      />
    </>
  );
}
