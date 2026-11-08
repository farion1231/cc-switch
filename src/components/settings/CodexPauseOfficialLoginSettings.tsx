import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";
import { toast } from "@/lib/toast";
import { SettingsSwitchRow } from "./SettingsLayout";

/**
 * 「暂停使用官方登录」：官方额度用完、Codex 锁住发送键时的应急开关。只在路由 / 聚合模式下
 * 生效，后端保存时当场重写 Codex 配置，Codex 要重启才读到。
 */
export function CodexPauseOfficialLoginSettings({
  settings,
  onChange,
}: {
  settings: SettingsFormState;
  onChange: (
    updates: Partial<SettingsFormState>,
  ) => void | boolean | Promise<void | boolean>;
}) {
  const { t } = useTranslation();
  const [saving, setSaving] = useState(false);

  const toggle = async (paused: boolean) => {
    setSaving(true);
    try {
      const saved = await onChange({ codexProxyWithoutOfficialLogin: paused });
      if (saved !== false) {
        toast.success(
          t(
            paused
              ? "settings.codexPauseOfficialLogin.pausedToast"
              : "settings.codexPauseOfficialLogin.resumedToast",
          ),
        );
      }
    } catch {
      toast.error(t("notifications.saveFailedGeneric"));
    } finally {
      setSaving(false);
    }
  };

  return (
    <SettingsSwitchRow
      label={t("settings.codexPauseOfficialLogin.title")}
      help={{
        title: t("settings.codexPauseOfficialLogin.title"),
        body: t("settings.codexPauseOfficialLogin.help"),
      }}
      checked={settings.codexProxyWithoutOfficialLogin ?? false}
      disabled={saving}
      onCheckedChange={(paused) => void toggle(paused)}
    />
  );
}
