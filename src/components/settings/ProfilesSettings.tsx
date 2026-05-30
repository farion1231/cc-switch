import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Copy, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import { generateUUID } from "@/utils/uuid";
import type { SettingsFormState } from "@/hooks/useSettings";
import type { Profile } from "@/types";

interface ProfilesSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

const PROFILE_DIR_FIELDS: Array<{
  key: keyof Pick<Profile, "claudeConfigDir" | "codexConfigDir">;
  labelKey: string;
  defaultLabel: string;
}> = [
  {
    key: "claudeConfigDir",
    labelKey: "settings.profiles.apps.claudeCode",
    defaultLabel: "Claude Code",
  },
  {
    key: "codexConfigDir",
    labelKey: "settings.profiles.apps.codex",
    defaultLabel: "Codex",
  },
];

const sanitize = (value?: string | null): string | undefined => {
  const trimmed = value?.trim() ?? "";
  return trimmed ? trimmed : undefined;
};

const currentProfileFromSettings = (
  id: string,
  label: string,
  settings: SettingsFormState,
): Profile => ({
  id,
  label,
  claudeConfigDir: sanitize(settings.claudeConfigDir),
  codexConfigDir: sanitize(settings.codexConfigDir),
  currentProviderClaude: settings.currentProviderClaude,
  currentProviderCodex: settings.currentProviderCodex,
});

export function ProfilesSettings({
  settings,
  onChange,
}: ProfilesSettingsProps) {
  const { t } = useTranslation();
  const defaultProfileName = t("settings.profiles.defaultName", {
    defaultValue: "Default",
  });
  const profiles = settings.profiles?.length
    ? settings.profiles
    : [currentProfileFromSettings("default", defaultProfileName, settings)];
  const persistedActiveProfileId = settings.activeProfileId ?? profiles[0]?.id;
  const [selectedProfileId, setSelectedProfileId] = useState<
    string | undefined
  >(persistedActiveProfileId);

  useEffect(() => {
    if (
      !selectedProfileId ||
      !profiles.some((profile) => profile.id === selectedProfileId)
    ) {
      setSelectedProfileId(persistedActiveProfileId);
    }
  }, [persistedActiveProfileId, profiles, selectedProfileId]);

  const selectedProfile =
    profiles.find((profile) => profile.id === selectedProfileId) ?? profiles[0];

  const profileIds = useMemo(
    () => new Set(profiles.map((profile) => profile.id)),
    [profiles],
  );

  const applyProfiles = (
    nextProfiles: Profile[],
    nextActiveProfileId = persistedActiveProfileId,
    nextSelectedProfileId = selectedProfileId,
  ) => {
    const nextActive = nextProfiles.find(
      (profile) => profile.id === nextActiveProfileId,
    );
    setSelectedProfileId(nextSelectedProfileId);
    onChange({
      profiles: nextProfiles,
      activeProfileId: nextActiveProfileId,
      ...(nextActive
        ? {
            claudeConfigDir: nextActive.claudeConfigDir,
            codexConfigDir: nextActive.codexConfigDir,
            currentProviderClaude: nextActive.currentProviderClaude,
            currentProviderCodex: nextActive.currentProviderCodex,
          }
        : {}),
    });
  };

  const updateProfile = (profileId: string, updates: Partial<Profile>) => {
    const nextProfiles = profiles.map((profile) =>
      profile.id === profileId ? { ...profile, ...updates } : profile,
    );
    applyProfiles(nextProfiles, persistedActiveProfileId, profileId);
  };

  const addProfile = () => {
    let baseId = `profile-${profiles.length + 1}`;
    while (profileIds.has(baseId)) {
      baseId = `profile-${generateUUID().slice(0, 8)}`;
    }
    const newProfile: Profile = {
      id: baseId,
      label: t("settings.profiles.defaultLabel", {
        number: profiles.length + 1,
        defaultValue: "Profile {{number}}",
      }),
    };
    applyProfiles(
      [...profiles, newProfile],
      persistedActiveProfileId ?? newProfile.id,
      newProfile.id,
    );
  };

  const duplicateProfile = (source: Profile) => {
    const id = `profile-${generateUUID().slice(0, 8)}`;
    const copy: Profile = {
      ...source,
      id,
      label: t("settings.profiles.copyLabel", {
        label: source.label,
        defaultValue: "{{label}} Copy",
      }),
    };
    applyProfiles([...profiles, copy], persistedActiveProfileId, copy.id);
  };

  const deleteProfile = (profileId: string) => {
    if (profiles.length <= 1) {
      return;
    }
    const nextProfiles = profiles.filter((profile) => profile.id !== profileId);
    const nextActiveProfileId =
      persistedActiveProfileId === profileId
        ? nextProfiles[0]?.id
        : persistedActiveProfileId;
    const nextSelectedProfileId =
      selectedProfileId === profileId ? nextActiveProfileId : selectedProfileId;
    applyProfiles(nextProfiles, nextActiveProfileId, nextSelectedProfileId);
  };

  if (!selectedProfile) {
    return null;
  }

  return (
    <div className="grid gap-5 lg:grid-cols-[220px_minmax(0,1fr)]">
      <section className="space-y-3">
        <div className="flex items-center justify-between gap-3">
          <div>
            <h3 className="text-sm font-medium">
              {t("settings.profiles.title", { defaultValue: "Profiles" })}
            </h3>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("settings.profiles.description", {
                defaultValue:
                  "Isolate local directories and current providers.",
              })}
            </p>
          </div>
          <Button
            type="button"
            size="icon"
            variant="outline"
            onClick={addProfile}
            title={t("settings.profiles.add", { defaultValue: "Add profile" })}
          >
            <Plus className="h-4 w-4" />
          </Button>
        </div>

        <div className="space-y-1">
          {profiles.map((profile) => {
            const active = profile.id === persistedActiveProfileId;
            const selected = profile.id === selectedProfile.id;
            return (
              <button
                key={profile.id}
                type="button"
                className={cn(
                  "flex w-full items-center justify-between gap-2 rounded-md border px-3 py-2 text-left text-sm transition-colors",
                  selected
                    ? "border-blue-500/50 bg-blue-500/10 text-foreground"
                    : "border-border-default bg-background hover:bg-muted/60",
                )}
                onClick={() => setSelectedProfileId(profile.id)}
              >
                <span className="min-w-0 flex-1 truncate">{profile.label}</span>
                {active ? (
                  <Badge variant="outline" className="shrink-0 px-1.5 py-0">
                    {t("settings.profiles.active", { defaultValue: "Active" })}
                  </Badge>
                ) : null}
              </button>
            );
          })}
        </div>
      </section>

      <section className="space-y-5">
        <div className="grid gap-4 sm:grid-cols-2">
          <div className="space-y-2">
            <Label htmlFor="profile-label">
              {t("settings.profiles.name", { defaultValue: "Name" })}
            </Label>
            <Input
              id="profile-label"
              value={selectedProfile.label}
              onChange={(event) =>
                updateProfile(selectedProfile.id, { label: event.target.value })
              }
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="profile-id">
              {t("settings.profiles.id", { defaultValue: "ID" })}
            </Label>
            <Input id="profile-id" value={selectedProfile.id} disabled />
          </div>
        </div>

        <div className="space-y-3">
          <div className="flex items-center justify-between gap-3">
            <div>
              <h4 className="text-sm font-medium">
                {t("settings.profiles.configDirectories", {
                  defaultValue: "Config directories",
                })}
              </h4>
              <p className="mt-1 text-xs text-muted-foreground">
                {t("settings.profiles.emptyHint", {
                  defaultValue:
                    "Empty fields use the default location for that app.",
                })}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => duplicateProfile(selectedProfile)}
              >
                <Copy className="h-3.5 w-3.5" />
                {t("settings.profiles.duplicate", {
                  defaultValue: "Duplicate",
                })}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() =>
                  applyProfiles(
                    profiles,
                    selectedProfile.id,
                    selectedProfile.id,
                  )
                }
              >
                <Check className="h-3.5 w-3.5" />
                {t("settings.profiles.setActive", {
                  defaultValue: "Set active",
                })}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={profiles.length <= 1}
                onClick={() => deleteProfile(selectedProfile.id)}
              >
                <Trash2 className="h-3.5 w-3.5" />
                {t("common.delete")}
              </Button>
            </div>
          </div>

          <div className="grid gap-3">
            {PROFILE_DIR_FIELDS.map((field) => (
              <div
                key={field.key}
                className="grid gap-2 sm:grid-cols-[120px_1fr] sm:items-center"
              >
                <Label className="text-xs text-muted-foreground">
                  {t(field.labelKey, { defaultValue: field.defaultLabel })}
                </Label>
                <Input
                  value={
                    (selectedProfile[field.key] as string | undefined) ?? ""
                  }
                  placeholder={t("settings.profiles.defaultPlaceholder", {
                    defaultValue: "Default",
                  })}
                  className="text-xs"
                  onChange={(event) =>
                    updateProfile(selectedProfile.id, {
                      [field.key]: sanitize(event.target.value),
                    } as Partial<Profile>)
                  }
                />
              </div>
            ))}
          </div>
        </div>
      </section>
    </div>
  );
}
