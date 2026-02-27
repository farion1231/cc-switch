import { useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";
import { AppWindow, MonitorUp, Power, EyeOff, Keyboard, X } from "lucide-react";
import { ToggleRow } from "@/components/ui/toggle-row";
import { settingsApi } from "@/lib/api";
import { toast } from "sonner";
import { AnimatePresence, motion } from "framer-motion";

/** Convert a KeyboardEvent into a Tauri-compatible shortcut string like "Ctrl+Shift+S" */
function keyEventToShortcut(e: React.KeyboardEvent): string | null {
  const ignoredKeys = new Set([
    "Control", "Shift", "Alt", "Meta", "CapsLock", "Tab", "Escape",
  ]);
  if (ignoredKeys.has(e.key)) return null;

  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (parts.length === 0) return null;

  let key = e.key;
  if (key === " ") key = "Space";
  else if (key.length === 1) key = key.toUpperCase();

  parts.push(key);
  return parts.join("+");
}

interface WindowSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

export function WindowSettings({ settings, onChange }: WindowSettingsProps) {
  const { t } = useTranslation();
  const [isRecording, setIsRecording] = useState(false);
  const [pendingKeys, setPendingKeys] = useState("");
  const inputRef = useRef<HTMLButtonElement>(null);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const shortcut = keyEventToShortcut(e);
      if (!shortcut) {
        const mods: string[] = [];
        if (e.ctrlKey || e.metaKey) mods.push("Ctrl");
        if (e.altKey) mods.push("Alt");
        if (e.shiftKey) mods.push("Shift");
        if (mods.length > 0) setPendingKeys(mods.join("+") + "+...");
        return;
      }

      setPendingKeys(shortcut);
      setIsRecording(false);

      (async () => {
        try {
          await settingsApi.registerGlobalShortcut(shortcut);
          onChange({ globalShortcut: shortcut });
        } catch (error) {
          console.error("Failed to set global shortcut:", error);
          toast.error(t("settings.globalShortcutFailed"));
          setPendingKeys("");
        }
      })();
    },
    [onChange, t],
  );

  const handleClear = useCallback(async () => {
    try {
      await settingsApi.unregisterGlobalShortcut();
      onChange({ globalShortcut: undefined });
      setPendingKeys("");
    } catch (error) {
      console.error("Failed to clear global shortcut:", error);
      toast.error(t("settings.globalShortcutFailed"));
    }
  }, [onChange, t]);

  const displayValue =
    isRecording
      ? pendingKeys || t("settings.globalShortcutRecording")
      : settings.globalShortcut ?? "";

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

        <div className="flex items-center justify-between gap-4 rounded-xl border border-border bg-card/50 p-4 transition-colors hover:bg-muted/50">
          <div className="flex items-center gap-3">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-background ring-1 ring-border">
              <Keyboard className="h-4 w-4 text-yellow-500" />
            </div>
            <div className="space-y-1">
              <p className="text-sm font-medium leading-none">
                {t("settings.globalShortcut")}
              </p>
              <p className="text-xs text-muted-foreground">
                {t("settings.globalShortcutDescription")}
              </p>
            </div>
          </div>
          <div className="flex items-center gap-1.5 shrink-0">
            <button
              ref={inputRef}
              type="button"
              className={`h-8 min-w-[160px] px-3 rounded-lg border text-sm text-left transition-colors ${
                isRecording
                  ? "border-primary bg-primary/5 ring-1 ring-primary/30"
                  : "border-border bg-background hover:border-border/80"
              }`}
              onClick={() => {
                setIsRecording(true);
                setPendingKeys("");
              }}
              onKeyDown={isRecording ? handleKeyDown : undefined}
              onBlur={() => {
                setIsRecording(false);
                setPendingKeys("");
              }}
            >
              <span className={displayValue ? "" : "text-muted-foreground"}>
                {displayValue || t("settings.globalShortcutNone")}
              </span>
            </button>
            {settings.globalShortcut && !isRecording && (
              <button
                type="button"
                className="h-8 w-8 flex items-center justify-center rounded-lg border border-border bg-background hover:bg-destructive/10 hover:border-destructive/30 transition-colors ring-1 ring-border"
                onClick={handleClear}
                title={t("settings.globalShortcutClear")}
              >
                <X className="h-3.5 w-3.5 text-muted-foreground" />
              </button>
            )}
          </div>
        </div>
      </div>
    </section>
  );
}
