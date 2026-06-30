import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";
import {
  RefreshCw,
  Download,
  Upload,
  BarChart3,
  Puzzle,
  Clock,
} from "lucide-react";
import { ToggleRow } from "@/components/ui/toggle-row";
import { Input } from "@/components/ui/input";

interface SyncAutomationSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

export function SyncAutomationSettings({
  settings,
  onChange,
}: SyncAutomationSettingsProps) {
  const { t } = useTranslation();

  const sessionDisabled = !settings.sessionUsageSyncEnabled;

  return (
    <section className="space-y-6">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <RefreshCw className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">
          {t("settings.syncAutomation.title")}
        </h3>
      </div>

      {/* Auto Import */}
      <div className="space-y-3">
        <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
          {t("settings.syncAutomation.autoImport")}
        </p>
        <ToggleRow
          icon={<Download className="h-4 w-4 text-blue-500" />}
          title={t("settings.syncAutomation.autoImportMcp")}
          description={t("settings.syncAutomation.autoImportMcpDesc")}
          checked={!!settings.autoImportMcpOnStartup}
          onCheckedChange={(value) =>
            onChange({ autoImportMcpOnStartup: value })
          }
        />
        <ToggleRow
          icon={<Download className="h-4 w-4 text-green-500" />}
          title={t("settings.syncAutomation.autoImportPrompts")}
          description={t("settings.syncAutomation.autoImportPromptsDesc")}
          checked={!!settings.autoImportPromptsOnStartup}
          onCheckedChange={(value) =>
            onChange({ autoImportPromptsOnStartup: value })
          }
        />
        {(!settings.autoImportMcpOnStartup ||
          !settings.autoImportPromptsOnStartup) && (
          <p className="text-xs text-muted-foreground pl-11">
            {t("settings.syncAutomation.autoImportDisabledHint")}
          </p>
        )}
      </div>

      {/* Live Sync */}
      <div className="space-y-3">
        <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
          {t("settings.syncAutomation.liveSync")}
        </p>
        <ToggleRow
          icon={<Upload className="h-4 w-4 text-orange-500" />}
          title={t("settings.syncAutomation.mcpLiveSync")}
          description={t("settings.syncAutomation.mcpLiveSyncDesc")}
          checked={settings.mcpLiveSyncEnabled !== false}
          onCheckedChange={(value) => onChange({ mcpLiveSyncEnabled: value })}
        />
        <ToggleRow
          icon={<Upload className="h-4 w-4 text-purple-500" />}
          title={t("settings.syncAutomation.promptLiveSync")}
          description={t("settings.syncAutomation.promptLiveSyncDesc")}
          checked={settings.promptLiveSyncEnabled !== false}
          onCheckedChange={(value) =>
            onChange({ promptLiveSyncEnabled: value })
          }
        />
        <ToggleRow
          icon={<Puzzle className="h-4 w-4 text-cyan-500" />}
          title={t("settings.syncAutomation.skillLiveSync")}
          description={t("settings.syncAutomation.skillLiveSyncDesc")}
          checked={settings.skillLiveSyncEnabled !== false}
          onCheckedChange={(value) => onChange({ skillLiveSyncEnabled: value })}
        />
      </div>

      {/* Session Usage */}
      <div className="space-y-3">
        <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
          {t("settings.syncAutomation.sessionUsage")}
        </p>
        <ToggleRow
          icon={<BarChart3 className="h-4 w-4 text-amber-500" />}
          title={t("settings.syncAutomation.sessionUsageSync")}
          description={t("settings.syncAutomation.sessionUsageSyncDesc")}
          checked={!!settings.sessionUsageSyncEnabled}
          onCheckedChange={(value) =>
            onChange({ sessionUsageSyncEnabled: value })
          }
        />
        <div className="flex items-center gap-3 pl-11">
          <Clock className="h-4 w-4 text-muted-foreground" />
          <span className="text-sm text-muted-foreground">
            {t("settings.syncAutomation.sessionUsageInterval")}
          </span>
          <Input
            type="number"
            min={10}
            className="w-24 h-8"
            value={settings.sessionUsageSyncIntervalSecs ?? 60}
            disabled={sessionDisabled}
            onChange={(e) =>
              onChange({
                sessionUsageSyncIntervalSecs: Math.max(
                  10,
                  parseInt(e.target.value) || 60,
                ),
              })
            }
          />
        </div>
        <ToggleRow
          icon={<BarChart3 className="h-4 w-4 text-violet-500" />}
          title={t("settings.syncAutomation.sessionUsageSyncClaude")}
          checked={settings.sessionUsageSyncClaude !== false}
          onCheckedChange={(value) =>
            onChange({ sessionUsageSyncClaude: value })
          }
          disabled={sessionDisabled}
        />
        <ToggleRow
          icon={<BarChart3 className="h-4 w-4 text-emerald-500" />}
          title={t("settings.syncAutomation.sessionUsageSyncCodex")}
          checked={settings.sessionUsageSyncCodex !== false}
          onCheckedChange={(value) =>
            onChange({ sessionUsageSyncCodex: value })
          }
          disabled={sessionDisabled}
        />
        <ToggleRow
          icon={<BarChart3 className="h-4 w-4 text-sky-500" />}
          title={t("settings.syncAutomation.sessionUsageSyncGemini")}
          checked={settings.sessionUsageSyncGemini !== false}
          onCheckedChange={(value) =>
            onChange({ sessionUsageSyncGemini: value })
          }
          disabled={sessionDisabled}
        />
        <ToggleRow
          icon={<BarChart3 className="h-4 w-4 text-rose-500" />}
          title={t("settings.syncAutomation.sessionUsageSyncOpencode")}
          checked={settings.sessionUsageSyncOpencode !== false}
          onCheckedChange={(value) =>
            onChange({ sessionUsageSyncOpencode: value })
          }
          disabled={sessionDisabled}
        />
      </div>
    </section>
  );
}
