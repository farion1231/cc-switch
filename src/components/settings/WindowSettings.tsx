import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";
import {
  AppWindow,
  MonitorUp,
  Power,
  EyeOff,
  Timer,
  Gauge,
} from "lucide-react";
import { ToggleRow } from "@/components/ui/toggle-row";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { AnimatePresence, motion } from "framer-motion";
import { isLinux } from "@/lib/platform";

interface WindowSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

export function WindowSettings({ settings, onChange }: WindowSettingsProps) {
  const { t } = useTranslation();
  const lightweightDelay = settings.autoLightweightDelayMinutes ?? 20;
  const setLightweightDelay = (value: number) => {
    const next = Math.min(1440, Math.max(1, Math.round(value)));
    onChange({ autoLightweightDelayMinutes: next });
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
              key="lightweight-startup"
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 10 }}
              transition={{ duration: 0.3 }}
            >
              <ToggleRow
                icon={<Gauge className="h-4 w-4 text-emerald-500" />}
                title={t("settings.lightweightOnStartup")}
                description={t("settings.lightweightOnStartupDescription")}
                checked={settings.lightweightOnStartup ?? true}
                onCheckedChange={(value) =>
                  onChange({ lightweightOnStartup: value })
                }
              />
            </motion.div>
          )}
        </AnimatePresence>

        <AnimatePresence initial={false}>
          {settings.launchOnStartup &&
            !(settings.lightweightOnStartup ?? true) && (
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
                  onCheckedChange={(value) =>
                    onChange({ silentStartup: value })
                  }
                />
              </motion.div>
            )}
        </AnimatePresence>

        <ToggleRow
          icon={<Timer className="h-4 w-4 text-sky-500" />}
          title={t("settings.autoLightweightAfterClose")}
          description={t("settings.autoLightweightAfterCloseDescription")}
          checked={settings.autoLightweightAfterClose ?? true}
          onCheckedChange={(value) =>
            onChange({ autoLightweightAfterClose: value })
          }
        />

        <AnimatePresence initial={false}>
          {(settings.autoLightweightAfterClose ?? true) && (
            <motion.div
              key="auto-lightweight-delay"
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 10 }}
              transition={{ duration: 0.3 }}
              className="rounded-xl border border-border bg-card/50 p-4"
            >
              <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                <div className="space-y-1">
                  <p className="text-sm font-medium leading-none">
                    {t("settings.autoLightweightDelay")}
                  </p>
                  <p className="text-xs text-muted-foreground">
                    {t("settings.autoLightweightDelayDescription")}
                  </p>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  {[10, 20].map((minutes) => (
                    <Button
                      key={minutes}
                      type="button"
                      size="sm"
                      variant={
                        lightweightDelay === minutes ? "default" : "outline"
                      }
                      onClick={() => setLightweightDelay(minutes)}
                    >
                      {t("settings.minutesShort", { count: minutes })}
                    </Button>
                  ))}
                  <div className="flex items-center gap-2">
                    <Input
                      className="h-8 w-24"
                      type="number"
                      min={1}
                      max={1440}
                      value={lightweightDelay}
                      onChange={(event) => {
                        const next = Number.parseInt(event.target.value, 10);
                        if (Number.isFinite(next)) {
                          setLightweightDelay(next);
                        }
                      }}
                    />
                    <span className="text-xs text-muted-foreground">
                      {t("settings.minutesUnit")}
                    </span>
                  </div>
                </div>
              </div>
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
