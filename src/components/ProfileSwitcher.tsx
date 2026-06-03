import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Layers, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import { settingsApi } from "@/lib/api";
import type { Settings } from "@/types";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

interface ProfileSwitcherProps {
  settings?: Settings;
}

export function ProfileSwitcher({ settings }: ProfileSwitcherProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [isSwitching, setIsSwitching] = useState(false);

  const profiles = settings?.profiles ?? [];
  const activeProfileId = settings?.activeProfileId ?? profiles[0]?.id;

  const activeProfile = useMemo(
    () => profiles.find((profile) => profile.id === activeProfileId),
    [profiles, activeProfileId],
  );

  if (profiles.length <= 1 || !activeProfileId) {
    return null;
  }

  const handleSwitch = async (nextProfileId: string) => {
    if (nextProfileId === activeProfileId || isSwitching) {
      return;
    }

    const nextProfile = profiles.find(
      (profile) => profile.id === nextProfileId,
    );
    try {
      setIsSwitching(true);
      const nextSettings = await settingsApi.switchProfile(nextProfileId);
      queryClient.setQueryData(["settings"], nextSettings);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings"] }),
        queryClient.invalidateQueries({ queryKey: ["providers"] }),
        queryClient.invalidateQueries({ queryKey: ["proxyStatus"] }),
        queryClient.invalidateQueries({ queryKey: ["proxyTakeoverStatus"] }),
      ]);
      toast.success(
        t("notifications.profileSwitchSuccess", {
          profile: nextProfile?.label ?? nextProfileId,
          defaultValue: "Switched to {{profile}}",
        }),
        { closeButton: true },
      );
    } catch (error) {
      console.error("[ProfileSwitcher] Failed to switch profile", error);
      toast.error(
        t("notifications.profileSwitchFailed", {
          error: (error as Error)?.message ?? String(error),
          defaultValue: "Switch profile failed: {{error}}",
        }),
      );
    } finally {
      setIsSwitching(false);
    }
  };

  return (
    <Select
      value={activeProfileId}
      onValueChange={(value) => void handleSwitch(value)}
      disabled={isSwitching}
    >
      <SelectTrigger
        className="h-8 w-[150px] rounded-lg bg-background/70 px-2.5 text-xs shadow-none"
        title={t("settings.profiles.switch", {
          defaultValue: "Switch profile",
        })}
      >
        <div className="flex min-w-0 items-center gap-2">
          {isSwitching ? (
            <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-muted-foreground" />
          ) : (
            <Layers className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          )}
          <SelectValue
            placeholder={t("settings.profiles.placeholder", {
              defaultValue: "Profile",
            })}
          >
            <span className="truncate">
              {activeProfile?.label ??
                t("settings.profiles.placeholder", {
                  defaultValue: "Profile",
                })}
            </span>
          </SelectValue>
        </div>
      </SelectTrigger>
      <SelectContent align="end" className="min-w-[180px]">
        {profiles.map((profile) => (
          <SelectItem key={profile.id} value={profile.id}>
            {profile.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
