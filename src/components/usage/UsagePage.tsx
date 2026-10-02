import { useTranslation } from "react-i18next";
import { ChartColumn } from "lucide-react";
import { useSettings } from "@/hooks/useSettings";
import type { SettingsFormState } from "@/hooks/useSettings";
import { HelpTip } from "@/components/ui/help-tip";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { UsageDashboard } from "./UsageDashboard";

/** 侧栏「用量统计」：原来是设置里的一个页签，不开路由也有数据（读会话日志）。 */
export function UsagePage() {
  const { t } = useTranslation();
  const { settings, updateSettings, autoSaveSettings } = useSettings();

  const save = (updates: Partial<SettingsFormState>) => {
    updateSettings(updates);
    void autoSaveSettings(updates).catch(() => undefined);
  };

  return (
    <>
      <AppPageHeader
        icon={<ChartColumn className="h-5 w-5" strokeWidth={1.5} />}
        title={t("nav.usage")}
        titleExtra={
          <HelpTip title={t("nav.usage")}>{t("usage.subtitle")}</HelpTip>
        }
      />
      <div id="main-content" className="min-h-0 flex-1 overflow-y-auto">
        <div className="px-6 pt-4">
          <UsageDashboard
            refreshIntervalMs={settings?.usageDashboardRefreshIntervalMs}
            onRefreshIntervalChange={(usageDashboardRefreshIntervalMs) =>
              save({ usageDashboardRefreshIntervalMs })
            }
            sessionAutoSyncEnabled={settings?.sessionAutoSyncEnabled ?? true}
            onSessionAutoSyncEnabledChange={(sessionAutoSyncEnabled) =>
              save({ sessionAutoSyncEnabled })
            }
          />
        </div>
      </div>
    </>
  );
}
