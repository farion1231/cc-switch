import { Switch } from "@/components/ui/switch";
import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";

interface WindowSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

export function WindowSettings({ settings, onChange }: WindowSettingsProps) {
  const { t } = useTranslation();

  return (
    <section className="space-y-4">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("settings.windowBehavior")}</h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.windowBehaviorHint")}
        </p>
      </header>

      <ToggleRow
        title={t("settings.launchOnStartup")}
        description={t("settings.launchOnStartupDescription")}
        checked={!!settings.launchOnStartup}
        onCheckedChange={(value) => onChange({ launchOnStartup: value })}
      />

      <ToggleRow
        title={t("settings.minimizeToTray")}
        description={t("settings.minimizeToTrayDescription")}
        checked={settings.minimizeToTrayOnClose}
        onCheckedChange={(value) => onChange({ minimizeToTrayOnClose: value })}
      />

      <ToggleRow
        title={t("settings.enableClaudePluginIntegration")}
        description={t("settings.enableClaudePluginIntegrationDescription")}
        checked={!!settings.enableClaudePluginIntegration}
        onCheckedChange={(value) =>
          onChange({ enableClaudePluginIntegration: value })
        }
      />

      <ToggleRow
        title={t("settings.skipClaudeOnboarding")}
        description={t("settings.skipClaudeOnboardingDescription")}
        checked={!!settings.skipClaudeOnboarding}
        onCheckedChange={(value) => onChange({ skipClaudeOnboarding: value })}
      />
    </section>
  );
}

interface ToggleRowProps {
  title: string;
  description?: string;
  checked: boolean;
  onCheckedChange: (value: boolean) => void;
}

function ToggleRow({
  title,
  description,
  checked,
  onCheckedChange,
}: ToggleRowProps) {
  return (
    <div className="flex items-start justify-between gap-4 rounded-lg border border-border-default p-4">
      <div className="space-y-1">
        <p className="text-sm font-medium leading-none">{title}</p>
        {description ? (
          <p className="text-xs text-muted-foreground">{description}</p>
        ) : null}
      </div>
      <Switch
        checked={checked}
        onCheckedChange={onCheckedChange}
        aria-label={title}
      />
    </div>
  );
}
