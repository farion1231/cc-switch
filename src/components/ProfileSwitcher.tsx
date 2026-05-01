import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Layers, Check } from "lucide-react";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { ConfigDirProfile } from "@/types";

interface ProfileSwitcherProps {
  profiles: ConfigDirProfile[];
  activeProfileId: string | undefined;
  onSwitchProfile: (id: string) => Promise<void>;
}

export function ProfileSwitcher({
  profiles,
  activeProfileId,
  onSwitchProfile,
}: ProfileSwitcherProps) {
  const { t } = useTranslation();

  const activeProfile = profiles.find((p) => p.id === activeProfileId);
  const displayName = activeProfile?.name || t("profileSwitcher.noProfile");

  const handleSwitch = async (id: string) => {
    if (id === activeProfileId) return;
    try {
      await onSwitchProfile(id);
      const newProfile = profiles.find((p) => p.id === id);
      toast.success(
        t("profileSwitcher.switchSuccess", {
          name: newProfile?.name || id,
        }),
      );
    } catch (error) {
      console.error("[ProfileSwitcher] Failed to switch profile", error);
      toast.error(t("profileSwitcher.switchFailed"));
    }
  };

  if (profiles.length === 0) return null;

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          className="gap-1.5 h-7"
          title={t("profileSwitcher.title")}
        >
          <Layers className="w-3.5 h-3.5 text-muted-foreground" />
          <span className="truncate max-w-[100px] text-xs font-medium">
            {displayName}
          </span>
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="p-1 min-w-[120px]">
        <div className="space-y-0.5">
          {profiles.map((profile) => (
            <button
              key={profile.id}
              type="button"
              onClick={() => handleSwitch(profile.id)}
              className={cn(
                "w-full flex items-center gap-2 rounded-md px-2 py-1.5 text-sm transition-colors",
                "hover:bg-accent hover:text-accent-foreground",
                profile.id === activeProfileId
                  ? "bg-accent/50 text-accent-foreground"
                  : "text-muted-foreground",
              )}
            >
              <div
                className={cn(
                  "flex h-4 w-4 items-center justify-center rounded-full border",
                  profile.id === activeProfileId
                    ? "border-primary bg-primary text-primary-foreground"
                    : "border-muted-foreground/30",
                )}
              >
                {profile.id === activeProfileId && (
                  <Check className="h-2.5 w-2.5" />
                )}
              </div>
              <span className="truncate">{profile.name}</span>
            </button>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  );
}