import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { settingsApi, type PrivacyFilterConfig } from "@/lib/api/settings";
import {
  SettingsCard,
  SettingsSwitchRow,
} from "@/components/settings/SettingsLayout";

export function PrivacyFilterConfigPanel() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<PrivacyFilterConfig>({
    enabled: false,
  });
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    settingsApi
      .getPrivacyFilterConfig()
      .then(setConfig)
      .catch((e) => console.error("Failed to load privacy filter config:", e))
      .finally(() => setIsLoading(false));
  }, []);

  const handleChange = async (updates: Partial<PrivacyFilterConfig>) => {
    const newConfig = { ...config, ...updates };
    setConfig(newConfig);
    try {
      await settingsApi.setPrivacyFilterConfig(newConfig);
    } catch (e) {
      console.error("Failed to save privacy filter config:", e);
      toast.error(String(e));
      setConfig(config);
    }
  };

  if (isLoading) return null;

  return (
    <SettingsCard>
      <SettingsSwitchRow
        label={t("settings.advanced.privacyFilter.enabled")}
        checked={config.enabled}
        onCheckedChange={(checked) => void handleChange({ enabled: checked })}
      />
    </SettingsCard>
  );
}
