import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { BellRing, ShieldAlert, Clock3, PlugZap, RefreshCcw } from "lucide-react";
import { ToggleRow } from "@/components/ui/toggle-row";
import { Button } from "@/components/ui/button";
import type { SettingsFormState } from "@/hooks/useSettings";
import { isWindows } from "@/lib/platform";
import { claudeNotifyApi, type ClaudeNotifyStatus } from "@/lib/api/claudeNotify";
import { toast } from "sonner";

interface ClaudeBackgroundNotificationSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

export function ClaudeBackgroundNotificationSettings({
  settings,
  onChange,
}: ClaudeBackgroundNotificationSettingsProps) {
  const { t } = useTranslation();
  const windowsOnly = isWindows();
  const enabled = !!settings.enableClaudeBackgroundNotifications;
  const showDetails = windowsOnly && enabled;
  const [status, setStatus] = useState<ClaudeNotifyStatus | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [isApplying, setIsApplying] = useState(false);
  const [isClearing, setIsClearing] = useState(false);

  const refreshStatus = async () => {
    if (!windowsOnly) {
      setStatus(null);
      return;
    }

    setIsLoading(true);
    try {
      const next = await claudeNotifyApi.getStatus();
      setStatus(next);
    } catch (error) {
      console.warn("[ClaudeBackgroundNotificationSettings] Failed to load status", error);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    if (!windowsOnly) {
      setStatus(null);
      return;
    }

    if (!enabled) {
      setStatus(null);
      return;
    }

    void refreshStatus();
    const timer = window.setTimeout(() => {
      void refreshStatus();
    }, 400);

    return () => {
      window.clearTimeout(timer);
    };
  }, [windowsOnly, enabled]);

  const runtimeStatusLabel = status?.listening
    ? t("settings.claudeNotifyListening")
    : t("settings.claudeNotifyStopped");
  const hooksStatusLabel = status?.hooksApplied
    ? t("settings.claudeNotifyHooksInstalled")
    : t("settings.claudeNotifyHooksNotInstalled");

  const applyHooks = async () => {
    if (!status?.port) {
      toast.error(t("settings.claudeNotifyPortUnavailable"));
      return;
    }

    setIsApplying(true);
    try {
      await claudeNotifyApi.applyHooks();
      setStatus((prev) => (prev ? { ...prev, hooksApplied: true } : prev));
      toast.success(t("settings.claudeNotifyHooksApplied"));
    } catch (error) {
      console.error("[ClaudeBackgroundNotificationSettings] Failed to apply hooks", error);
      toast.error(t("settings.claudeNotifyHooksApplyFailed"));
    } finally {
      setIsApplying(false);
    }
  };

  const clearHooks = async () => {
    setIsClearing(true);
    try {
      await claudeNotifyApi.clearHooks();
      setStatus((prev) => (prev ? { ...prev, hooksApplied: false } : prev));
      toast.success(t("settings.claudeNotifyHooksCleared"));
    } catch (error) {
      console.error("[ClaudeBackgroundNotificationSettings] Failed to clear hooks", error);
      toast.error(t("settings.claudeNotifyHooksClearFailed"));
    } finally {
      setIsClearing(false);
    }
  };

  return (
    <section className="space-y-3">
      {windowsOnly ? (
        <>
          <ToggleRow
            icon={<BellRing className="h-4 w-4 text-amber-500" />}
            title={t("settings.enableClaudeBackgroundNotifications")}
            description={t("settings.enableClaudeBackgroundNotificationsDescription")}
            checked={enabled}
            onCheckedChange={(value) =>
              onChange({
                enableClaudeBackgroundNotifications: value,
                enableClaudePermissionPromptNotifications:
                  settings.enableClaudePermissionPromptNotifications ?? true,
                enableClaudeRoundCompleteNotifications:
                  settings.enableClaudeRoundCompleteNotifications ?? true,
              })
            }
          />

          {showDetails ? (
            <>
              <ToggleRow
                icon={<ShieldAlert className="h-4 w-4 text-rose-500" />}
                title={t("settings.enableClaudePermissionPromptNotifications")}
                description={t("settings.enableClaudePermissionPromptNotificationsDescription")}
                checked={!!settings.enableClaudePermissionPromptNotifications}
                onCheckedChange={(value) =>
                  onChange({ enableClaudePermissionPromptNotifications: value })
                }
              />

              <ToggleRow
                icon={<Clock3 className="h-4 w-4 text-sky-500" />}
                title={t("settings.enableClaudeRoundCompleteNotifications")}
                description={t("settings.enableClaudeRoundCompleteNotificationsDescription")}
                checked={!!settings.enableClaudeRoundCompleteNotifications}
                onCheckedChange={(value) =>
                  onChange({ enableClaudeRoundCompleteNotifications: value })
                }
              />

              <div className="rounded-xl border border-border bg-card/50 p-4 space-y-3">
                <div className="flex items-center gap-2 text-sm font-medium">
                  <PlugZap className="h-4 w-4 text-violet-500" />
                  <span>{t("settings.claudeNotifyIntegrationTitle")}</span>
                </div>

                <div className="space-y-1 text-xs text-muted-foreground">
                  <p>{t("settings.claudeNotifyRuntimeStatus", { status: runtimeStatusLabel })}</p>
                  <p>
                    {t("settings.claudeNotifyPortLabel", {
                      port: status?.port ?? "-",
                    })}
                  </p>
                  <p>{t("settings.claudeNotifyHooksStatus", { status: hooksStatusLabel })}</p>
                </div>

                <div className="flex flex-wrap gap-2">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => void refreshStatus()}
                    disabled={isLoading}
                  >
                    <RefreshCcw className="h-3.5 w-3.5" />
                    {t("settings.claudeNotifyRefresh")}
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => void applyHooks()}
                    disabled={!enabled || isApplying || !status?.port}
                  >
                    {t("settings.claudeNotifyApplyHooks")}
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => void clearHooks()}
                    disabled={isClearing || !status?.hooksApplied}
                  >
                    {t("settings.claudeNotifyClearHooks")}
                  </Button>
                </div>
              </div>
            </>
          ) : null}
        </>
      ) : (
        <div className="rounded-xl border border-dashed border-border bg-card/30 p-4 text-sm text-muted-foreground">
          {t("settings.claudeNotifyWindowsOnlyNotice")}
        </div>
      )}
    </section>
  );
}
