import { Activity, Clock, Gauge } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";
import { ToggleRow } from "@/components/ui/toggle-row";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

interface UsageSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

export function UsageSettings({ settings, onChange }: UsageSettingsProps) {
  const { t } = useTranslation();
  const refreshInterval = String(settings.codexQuotaRefreshInterval ?? 300);

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 border-b border-border/40 pb-2">
        <Activity className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">
          {t("settings.usageQuery", { defaultValue: "Usage query" })}
        </h3>
      </div>

      <div className="space-y-3">
        <ToggleRow
          icon={<Clock className="h-4 w-4 text-emerald-500" />}
          title={t("settings.usageAutoRefresh", {
            defaultValue: "Auto refresh usage",
          })}
          description={t("settings.usageAutoRefreshDescription", {
            defaultValue: "Refresh Codex account usage in the background.",
          })}
          checked={settings.usageAutoRefresh ?? true}
          onCheckedChange={(value) => onChange({ usageAutoRefresh: value })}
        />

        <ToggleRow
          icon={<Gauge className="h-4 w-4 text-amber-500" />}
          title={t("settings.usageAdaptiveRefresh", {
            defaultValue: "Adaptive refresh",
          })}
          description={t("settings.usageAdaptiveRefreshDescription", {
            defaultValue:
              "Refresh faster during Codex activity and back off while idle.",
          })}
          checked={settings.usageAdaptiveRefresh ?? false}
          onCheckedChange={(value) => onChange({ usageAdaptiveRefresh: value })}
        />

        <div className="flex items-center justify-between gap-4 rounded-lg border bg-card p-4">
          <div className="min-w-0">
            <div className="text-sm font-medium">
              {t("settings.usageRefreshInterval", {
                defaultValue: "Refresh interval",
              })}
            </div>
            <div className="text-xs text-muted-foreground">
              {t("settings.usageRefreshIntervalDescription", {
                defaultValue: "Used by background and tray usage refresh.",
              })}
            </div>
          </div>
          <Select
            value={refreshInterval}
            onValueChange={(value) =>
              onChange({ codexQuotaRefreshInterval: Number(value) })
            }
            disabled={settings.usageAdaptiveRefresh ?? false}
          >
            <SelectTrigger className="h-9 w-[120px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="30">30s</SelectItem>
              <SelectItem value="60">1min</SelectItem>
              <SelectItem value="300">5min</SelectItem>
              <SelectItem value="1800">30min</SelectItem>
            </SelectContent>
          </Select>
        </div>

        <ToggleRow
          icon={<Activity className="h-4 w-4 text-blue-500" />}
          title={t("settings.usageShowAllAccounts", {
            defaultValue: "Show all Codex accounts",
          })}
          description={t("settings.usageShowAllAccountsDescription", {
            defaultValue:
              "Show the multi-account quota panel on the Codex page.",
          })}
          checked={settings.usageShowAllAccounts ?? true}
          onCheckedChange={(value) => onChange({ usageShowAllAccounts: value })}
        />
      </div>
    </section>
  );
}
