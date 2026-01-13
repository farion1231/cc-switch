import { Switch } from "@/components/ui/switch";
import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";
import { AppWindow, MonitorUp, Power } from "lucide-react";

interface WindowSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

export function WindowSettings({ settings, onChange }: WindowSettingsProps) {
  const { t } = useTranslation();

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <AppWindow className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">{t("settings.windowBehavior")}</h3>
      </div>

      <div className="space-y-3">
        <ToggleRow
          icon={<Power className="h-4 w-4 text-orange-500" />}
          title={t("settings.launchOnStartup")}
          description={t("settings.launchOnStartupDescription")}
          checked={!!settings.launchOnStartup}
          onCheckedChange={(value) => onChange({ launchOnStartup: value })}
        />

        <ToggleRow
          icon={<AppWindow className="h-4 w-4 text-blue-500" />}
          title={t("settings.minimizeToTray")}
          description={t("settings.minimizeToTrayDescription")}
          checked={settings.minimizeToTrayOnClose}
          onCheckedChange={(value) =>
            onChange({ minimizeToTrayOnClose: value })
          }
        />

        <ToggleRow
          icon={<MonitorUp className="h-4 w-4 text-purple-500" />}
          title={t("settings.enableClaudePluginIntegration")}
          description={t("settings.enableClaudePluginIntegrationDescription")}
          checked={!!settings.enableClaudePluginIntegration}
          onCheckedChange={(value) =>
            onChange({ enableClaudePluginIntegration: value })
          }
        />

        <ToggleRow
          icon={<MonitorUp className="h-4 w-4 text-cyan-500" />}
          title={t("settings.skipClaudeOnboarding")}
          description={t("settings.skipClaudeOnboardingDescription")}
          checked={!!settings.skipClaudeOnboarding}
          onCheckedChange={(value) => onChange({ skipClaudeOnboarding: value })}
        />
      </div>
    </section>
  );
}

interface ToggleRowProps {
  icon: React.ReactNode;
  title: string;
  description?: string;
  checked: boolean;
  onCheckedChange: (value: boolean) => void;
}

function ToggleRow({
  icon,
  title,
  description,
  checked,
  onCheckedChange,
}: ToggleRowProps) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-xl border border-border bg-card/50 p-4 transition-colors hover:bg-muted/50">
      <div className="flex items-center gap-3">
        <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-background ring-1 ring-border">
          {icon}
        </div>
        <div className="space-y-1">
          <p className="text-sm font-medium leading-none">{title}</p>
          {description ? (
            <p className="text-xs text-muted-foreground">{description}</p>
          ) : null}
        </div>
      </div>
      <Switch
        checked={checked}
        onCheckedChange={onCheckedChange}
        aria-label={title}
      />
    </div>
  );
}
