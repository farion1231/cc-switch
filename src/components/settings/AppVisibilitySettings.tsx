import { useTranslation } from "react-i18next";
import { FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ToggleRow } from "@/components/ui/toggle-row";
import { cn } from "@/lib/utils";
import type { SettingsFormState } from "@/hooks/useSettings";
import type { VisibleApps } from "@/types";
import type { AppId } from "@/lib/api";
import {
  APP_ICON_MAP,
  APP_IDS,
  DEFAULT_VISIBLE_APPS,
} from "@/config/appConfig";

interface AppVisibilitySettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

const APP_NAME_KEYS: Record<AppId, string> = {
  claude: "apps.claudeCode",
  "claude-desktop": "apps.claudeDesktop",
  codex: "apps.codex",
  gemini: "apps.gemini",
  grokbuild: "apps.grokbuild",
  opencode: "apps.opencode",
  openclaw: "apps.openclaw",
  hermes: "apps.hermes",
  pi: "apps.pi",
  cursor: "apps.cursor",
};

export function AppVisibilitySettings({
  settings,
  onChange,
}: AppVisibilitySettingsProps) {
  const { t } = useTranslation();

  const visibleApps: VisibleApps = settings.visibleApps ?? DEFAULT_VISIBLE_APPS;

  // Count how many apps are currently visible
  const visibleCount = Object.values(visibleApps).filter(Boolean).length;

  const handleToggle = (appId: AppId) => {
    const isCurrentlyVisible = visibleApps[appId];
    // Prevent disabling the last visible app
    if (isCurrentlyVisible && visibleCount <= 1) return;

    onChange({
      visibleApps: {
        ...visibleApps,
        [appId]: !isCurrentlyVisible,
      },
    });
  };

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">
          {t("settings.appVisibility.title")}
        </h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.appVisibility.description")}
        </p>
      </header>
      <div className="flex flex-wrap gap-1 rounded-md border border-border-default bg-background p-1">
        {APP_IDS.map((appId) => {
          const isVisible = visibleApps[appId];
          const name = t(APP_NAME_KEYS[appId]);
          // Disable button if this is the last visible app
          const isDisabled = isVisible && visibleCount <= 1;

          return (
            <AppButton
              key={appId}
              active={isVisible}
              disabled={isDisabled}
              onClick={() => handleToggle(appId)}
              icon={APP_ICON_MAP[appId].icon}
            >
              {name}
            </AppButton>
          );
        })}
      </div>
      <ToggleRow
        icon={<FolderOpen className="h-4 w-4 text-emerald-500" />}
        title={t("settings.appVisibility.showProfileSwitcher")}
        description={t("settings.appVisibility.showProfileSwitcherDescription")}
        checked={settings.showProfileSwitcher ?? true}
        onCheckedChange={(value) => onChange({ showProfileSwitcher: value })}
      />
    </section>
  );
}

interface AppButtonProps {
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  children: React.ReactNode;
}

function AppButton({
  active,
  disabled,
  onClick,
  icon,
  children,
}: AppButtonProps) {
  return (
    <Button
      type="button"
      onClick={onClick}
      disabled={disabled}
      size="sm"
      variant={active ? "default" : "ghost"}
      className={cn(
        "min-w-[90px] w-auto gap-1.5 px-3",
        active
          ? "shadow-sm"
          : "text-muted-foreground hover:text-foreground hover:bg-muted",
      )}
    >
      <span aria-hidden="true">{icon}</span>
      {children}
    </Button>
  );
}
