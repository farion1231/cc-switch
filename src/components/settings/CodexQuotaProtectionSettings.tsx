import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { toast } from "@/lib/toast";
import { SettingsRow, SettingsSwitchRow } from "./SettingsLayout";

export const DEFAULT_CODEX_QUOTA_PROTECTION = {
  enabled: true,
  fiveHourThreshold: 95,
  weeklyThreshold: 98,
};

export function CodexQuotaProtectionSettings({
  settings,
  onChange,
}: {
  settings: SettingsFormState;
  onChange: (
    updates: Partial<SettingsFormState>,
  ) => void | boolean | Promise<void | boolean>;
}) {
  const { t } = useTranslation();
  const id = useId();
  const config =
    settings.codexQuotaProtection ?? DEFAULT_CODEX_QUOTA_PROTECTION;
  // Keep edits separate from optimistic settings and their possible rollback.
  const [fiveHourDraft, setFiveHourDraft] = useState<string>();
  const [weeklyDraft, setWeeklyDraft] = useState<string>();
  const fiveHour = fiveHourDraft ?? String(config.fiveHourThreshold);
  const weekly = weeklyDraft ?? String(config.weeklyThreshold);
  const [saving, setSaving] = useState(false);
  const valid = (value: string) =>
    /^\d+$/.test(value) && Number(value) >= 1 && Number(value) <= 99;
  const invalid = !valid(fiveHour) || !valid(weekly);
  const dirty =
    Number(fiveHour) !== config.fiveHourThreshold ||
    Number(weekly) !== config.weeklyThreshold;

  const save = async (next: typeof DEFAULT_CODEX_QUOTA_PROTECTION) => {
    setSaving(true);
    try {
      const saved = await onChange({ codexQuotaProtection: next });
      if (saved !== false) {
        setFiveHourDraft(undefined);
        setWeeklyDraft(undefined);
      }
    } catch {
      toast.error(t("notifications.saveFailedGeneric"));
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <SettingsSwitchRow
        label={t("settings.codexQuotaProtection.title")}
        help={{
          title: t("settings.codexQuotaProtection.title"),
          body: t("settings.codexQuotaProtection.help"),
        }}
        checked={config.enabled}
        disabled={saving}
        onCheckedChange={(enabled) => void save({ ...config, enabled })}
      />
      {config.enabled && (
        <SettingsRow label={t("settings.codexQuotaProtection.thresholds")}>
          <div className="flex flex-wrap items-end gap-3">
            <label
              htmlFor={`${id}-5h`}
              className="space-y-1 text-caption text-fg-2"
            >
              <span>{t("settings.codexQuotaProtection.fiveHour")}</span>
              <Input
                id={`${id}-5h`}
                type="number"
                min={1}
                max={99}
                step={1}
                value={fiveHour}
                disabled={saving}
                aria-invalid={!valid(fiveHour)}
                aria-describedby={`${id}-hint`}
                onChange={(event) => setFiveHourDraft(event.target.value)}
                className="w-24"
              />
            </label>
            <label
              htmlFor={`${id}-weekly`}
              className="space-y-1 text-caption text-fg-2"
            >
              <span>{t("settings.codexQuotaProtection.weekly")}</span>
              <Input
                id={`${id}-weekly`}
                type="number"
                min={1}
                max={99}
                step={1}
                value={weekly}
                disabled={saving}
                aria-invalid={!valid(weekly)}
                aria-describedby={`${id}-hint`}
                onChange={(event) => setWeeklyDraft(event.target.value)}
                className="w-24"
              />
            </label>
            <Button
              variant="solid"
              size="compact"
              disabled={saving || invalid || !dirty}
              onClick={() =>
                void save({
                  ...config,
                  fiveHourThreshold: Number(fiveHour),
                  weeklyThreshold: Number(weekly),
                })
              }
            >
              {t("settings.codexQuotaProtection.save")}
            </Button>
            <Button
              variant="outline"
              size="compact"
              disabled={saving}
              onClick={() => {
                void save({
                  ...DEFAULT_CODEX_QUOTA_PROTECTION,
                  enabled: config.enabled,
                });
              }}
            >
              {t("settings.codexQuotaProtection.reset")}
            </Button>
          </div>
          <p
            id={`${id}-hint`}
            className="mt-2 text-caption text-fg-2"
            role={invalid ? "alert" : undefined}
          >
            {t(
              invalid
                ? "settings.codexQuotaProtection.invalid"
                : "settings.codexQuotaProtection.hint",
            )}
          </p>
        </SettingsRow>
      )}
    </>
  );
}
