import { History, Server, Settings2, Wrench } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ToggleRow } from "@/components/ui/toggle-row";
import type { SettingsFormState } from "@/hooks/useSettings";
import type { ManagementScope } from "@/types";

interface ManagementScopeSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void | Promise<unknown>;
}

const DEFAULT_SCOPE: ManagementScope = {
  mcp: true,
  skills: true,
  sessions: true,
};

export function ManagementScopeSettings({
  settings,
  onChange,
}: ManagementScopeSettingsProps) {
  const { t } = useTranslation();
  const scope = settings.managementScope ?? DEFAULT_SCOPE;
  const providersOnly = !scope.mcp && !scope.skills && !scope.sessions;

  const updateScope = (updates: Partial<ManagementScope>) =>
    onChange({ managementScope: { ...scope, ...updates } });

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <Settings2 className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">
          {t("settings.managementScope.title")}
        </h3>
      </div>

      <ToggleRow
        icon={<Settings2 className="h-4 w-4 text-orange-500" />}
        title={t("settings.managementScope.providersOnly")}
        description={t("settings.managementScope.providersOnlyDescription")}
        checked={providersOnly}
        onCheckedChange={(checked) =>
          onChange({
            managementScope: checked
              ? { mcp: false, skills: false, sessions: false }
              : { ...DEFAULT_SCOPE },
          })
        }
      />

      <div className="space-y-3 pl-2 border-l border-border/50">
        <ToggleRow
          icon={<Server className="h-4 w-4 text-cyan-500" />}
          title={t("settings.managementScope.mcp")}
          description={t("settings.managementScope.mcpDescription")}
          checked={scope.mcp}
          onCheckedChange={(checked) => updateScope({ mcp: checked })}
        />
        <ToggleRow
          icon={<Wrench className="h-4 w-4 text-emerald-500" />}
          title={t("settings.managementScope.skills")}
          description={t("settings.managementScope.skillsDescription")}
          checked={scope.skills}
          onCheckedChange={(checked) => updateScope({ skills: checked })}
        />
        <ToggleRow
          icon={<History className="h-4 w-4 text-sky-500" />}
          title={t("settings.managementScope.sessions")}
          description={t("settings.managementScope.sessionsDescription")}
          checked={scope.sessions}
          onCheckedChange={(checked) => updateScope({ sessions: checked })}
        />
      </div>
    </section>
  );
}
