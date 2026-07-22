import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";
import { AppWindow, MonitorUp, Power, EyeOff, Gauge } from "lucide-react";
import { ToggleRow } from "@/components/ui/toggle-row";
import { AnimatePresence, motion } from "framer-motion";
import { isLinux } from "@/lib/platform";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

interface WindowSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

const DEFAULT_AUTO_LIGHTWEIGHT_MINUTES = 5;
const MAX_AUTO_LIGHTWEIGHT_MINUTES = 24 * 60;

// 0 disables the feature, matching the toggle and the backend normalization.
const clampAutoLightweightMinutes = (value: number): number => {
  if (!Number.isFinite(value)) return DEFAULT_AUTO_LIGHTWEIGHT_MINUTES;
  if (Math.trunc(value) <= 0) return 0;
  return Math.min(MAX_AUTO_LIGHTWEIGHT_MINUTES, Math.trunc(value));
};

export function WindowSettings({ settings, onChange }: WindowSettingsProps) {
  const { t } = useTranslation();
  const autoLightweightMinutes = settings.autoLightweightIdleMinutes ?? 0;
  const autoLightweightEnabled = autoLightweightMinutes > 0;
  const [autoLightweightDraft, setAutoLightweightDraft] = useState(
    String(autoLightweightMinutes || DEFAULT_AUTO_LIGHTWEIGHT_MINUTES),
  );

  useEffect(() => {
    setAutoLightweightDraft(
      String(autoLightweightMinutes || DEFAULT_AUTO_LIGHTWEIGHT_MINUTES),
    );
  }, [autoLightweightMinutes]);

  const saveAutoLightweightMinutes = () => {
    if (!autoLightweightEnabled) return;
    if (autoLightweightDraft.trim() === "") {
      setAutoLightweightDraft(String(autoLightweightMinutes));
      return;
    }
    const next = clampAutoLightweightMinutes(Number(autoLightweightDraft));
    setAutoLightweightDraft(String(next));
    onChange({ autoLightweightIdleMinutes: next });
  };

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

        <AnimatePresence initial={false}>
          {settings.launchOnStartup && (
            <motion.div
              key="silent-startup"
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 10 }}
              transition={{ duration: 0.3 }}
            >
              <ToggleRow
                icon={<EyeOff className="h-4 w-4 text-green-500" />}
                title={t("settings.silentStartup")}
                description={t("settings.silentStartupDescription")}
                checked={!!settings.silentStartup}
                onCheckedChange={(value) => onChange({ silentStartup: value })}
              />
            </motion.div>
          )}
        </AnimatePresence>

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
          icon={<Gauge className="h-4 w-4 text-emerald-500" />}
          title={t("settings.autoLightweight")}
          description={t("settings.autoLightweightDescription")}
          checked={autoLightweightEnabled}
          onCheckedChange={(value) =>
            onChange({
              autoLightweightIdleMinutes: value
                ? autoLightweightMinutes || DEFAULT_AUTO_LIGHTWEIGHT_MINUTES
                : 0,
            })
          }
        />

        <AnimatePresence initial={false}>
          {autoLightweightEnabled && (
            <motion.div
              key="auto-lightweight-minutes"
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 10 }}
              transition={{ duration: 0.3 }}
              className="flex items-center justify-between gap-4 rounded-xl border border-border bg-card/50 p-4"
            >
              <div className="space-y-1">
                <Label htmlFor="auto-lightweight-minutes" className="text-sm">
                  {t("settings.autoLightweightMinutes")}
                </Label>
                <p className="text-xs text-muted-foreground">
                  {t("settings.autoLightweightMinutesDescription")}
                </p>
              </div>
              <Input
                id="auto-lightweight-minutes"
                type="number"
                min={0}
                max={MAX_AUTO_LIGHTWEIGHT_MINUTES}
                step={1}
                value={autoLightweightDraft}
                onChange={(event) =>
                  setAutoLightweightDraft(event.target.value)
                }
                onBlur={saveAutoLightweightMinutes}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.currentTarget.blur();
                  }
                }}
                className="w-24"
              />
            </motion.div>
          )}
        </AnimatePresence>

        {isLinux() && (
          <ToggleRow
            icon={<AppWindow className="h-4 w-4 text-amber-500" />}
            title={t("settings.useAppWindowControls")}
            description={t("settings.useAppWindowControlsDescription")}
            checked={!!settings.useAppWindowControls}
            onCheckedChange={(value) =>
              onChange({ useAppWindowControls: value })
            }
          />
        )}
      </div>
    </section>
  );
}
