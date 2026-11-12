import { useState } from "react";
import { useTranslation } from "react-i18next";
import { copilotVscodeApi } from "@/lib/api/copilotVscode";
import type { Settings } from "@/types";
import { toast } from "@/lib/toast";
import { extractErrorMessage } from "@/utils/errorUtils";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import {
  SettingsBlock,
  SettingsCard,
  SettingsSwitchRow,
} from "./SettingsLayout";

export function CopilotVscodeSettings({
  settings,
  isSaving,
  onAutoSave,
}: {
  settings: Settings;
  isSaving: boolean;
  onAutoSave: (updates: Pick<Settings, "copilotVscode">) => Promise<boolean>;
}) {
  const { t } = useTranslation();
  const options = settings.copilotVscode ?? { enabled: false, targets: [] };
  const [working, setWorking] = useState(false);
  const busy = working || isSaving;

  const save = async (enabled: boolean) => {
    if (busy) return;
    setWorking(true);
    try {
      let targets = options.targets;
      if (enabled) {
        const profiles = await copilotVscodeApi.targets();
        targets = Array.from(
          new Set(
            profiles.map((profile) => profile.resources.languageModelsPath),
          ),
        );
        if (targets.length === 0) {
          toast.error(t("settings.copilotVscode.noTargets"));
          return;
        }
      }
      await onAutoSave({ copilotVscode: { enabled, targets } });
    } catch (error) {
      toast.error(t("settings.copilotVscode.failed"), {
        description: extractErrorMessage(error),
      });
    } finally {
      setWorking(false);
    }
  };

  return (
    <SettingsBlock
      id="app-config-copilot"
      title={
        <span className="flex items-center gap-2">
          <AppGlyph app="copilot" size={16} badgeClassName="bg-app" />
          {APP_DISPLAY_NAME.copilot}
        </span>
      }
    >
      <SettingsCard>
        <SettingsSwitchRow
          label={t("settings.copilotVscode.enable")}
          help={{
            title: t("settings.copilotVscode.enable"),
            body: t("settings.copilotVscode.description"),
          }}
          checked={options.enabled}
          disabled={busy}
          onCheckedChange={(enabled) => void save(enabled)}
        />
      </SettingsCard>
    </SettingsBlock>
  );
}
