/**
 * 首页「模型层级路由」编辑器。
 *
 * 默认展示路由方案列表；点击方案切换生效，点击编辑按钮进入详情后编辑每个 Claude 层级
 * （Opus/Sonnet/Haiku/Fable）的 provider、真实上游模型名、展示名和 1M 声明。
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowLeft,
  CheckCircle2,
  Copy,
  Pencil,
  Plus,
  Trash2,
} from "lucide-react";
import { providersApi } from "@/lib/api/providers";
import type {
  ModelTierRoutingConfig,
  ModelTierRoutingProfile,
  TierRoute,
} from "@/lib/api/settings";
import {
  getActiveModelTierRoutingProfile,
  getModelTierRoutingProfiles,
  MODEL_TIER_ROUTING_APPS,
  normalizeModelTierRoutingConfig,
  type ModelTierRoutingApp,
} from "@/hooks/useModelTierRouting";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import type { Provider } from "@/types";
import { supportsRouting } from "@/utils/providerRouting";
import { cn } from "@/lib/utils";

const TIERS = ["opus", "sonnet", "haiku", "fable"] as const;
type TierKey = (typeof TIERS)[number];

// Radix Select 不允许 value=""（空串），用哨兵值代表「未配置/使用默认」。
const NONE_PROVIDER = "__none__";

// 1M 能力声明的旧式后缀；新版改用显式 supports1m 字段，这里仅用于回退读取/迁移写入。
const ONE_M_SUFFIX = /\s*\[1m\]$/i;

function stripOneMSuffix(model: string): string {
  return model.replace(ONE_M_SUFFIX, "").trimEnd();
}

function readRoute(
  profile: ModelTierRoutingProfile,
  appId: ModelTierRoutingApp,
  tier: TierKey,
): {
  providerId: string;
  model: string;
  displayName: string;
  supports1m: boolean;
} {
  const route = profile.routes?.[appId]?.[tier];
  const rawModel = route?.model ?? "";
  return {
    providerId: route?.providerId ?? "",
    model: stripOneMSuffix(rawModel),
    displayName: route?.displayName ?? "",
    supports1m: route?.supports1m ?? ONE_M_SUFFIX.test(rawModel),
  };
}

function writeRoute(
  config: ModelTierRoutingConfig,
  appId: ModelTierRoutingApp,
  profileId: string,
  tier: TierKey,
  next: {
    providerId: string;
    model: string;
    displayName: string;
    supports1m: boolean;
  },
): ModelTierRoutingConfig {
  const normalized = normalizeModelTierRoutingConfig(config);
  const profiles = (normalized.profiles ?? []).map((profile) => {
    if (profile.id !== profileId) return profile;

    const appRoutes = { ...(profile.routes?.[appId] ?? {}) };
    if (!next.providerId) {
      delete appRoutes[tier];
    } else {
      const route: TierRoute = {
        providerId: next.providerId,
        model: stripOneMSuffix(next.model.trim()),
        displayName: next.displayName.trim(),
        supports1m: next.supports1m,
      };
      appRoutes[tier] = route;
    }

    return {
      ...profile,
      routes: { ...profile.routes, [appId]: appRoutes },
    };
  });

  return { ...normalized, profiles };
}

interface Props {
  appId: ModelTierRoutingApp;
  config: ModelTierRoutingConfig;
  onChange: (next: ModelTierRoutingConfig) => void;
}

export function ModelTierRoutingEditor({ appId, config, onChange }: Props) {
  const { t } = useTranslation();
  const [providers, setProviders] = useState<Record<string, Provider>>({});
  const [editingProfileId, setEditingProfileId] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] =
    useState<ModelTierRoutingProfile | null>(null);

  const normalizedConfig = normalizeModelTierRoutingConfig(config);
  const profiles = getModelTierRoutingProfiles(normalizedConfig);
  const activeProfile = getActiveModelTierRoutingProfile(
    normalizedConfig,
    appId,
  );
  const editingProfile =
    profiles.find((profile) => profile.id === editingProfileId) ?? null;

  useEffect(() => {
    providersApi
      .getAll(appId)
      .then((map) => setProviders(map ?? {}))
      .catch((e) => console.error("Failed to load providers:", e));
  }, [appId]);

  const providerList = Object.values(providers).sort(
    (a, b) => (a.sortIndex ?? 0) - (b.sortIndex ?? 0),
  );
  const routableProviders = providerList.filter((p) => supportsRouting(p));

  const profileDisplayName = (
    profile: ModelTierRoutingProfile,
    index = profiles.findIndex((item) => item.id === profile.id),
  ) =>
    profile.name.trim() ||
    t("home.modelTierRouting.profileFallbackName", {
      index: index >= 0 ? index + 1 : 1,
      defaultValue: `Profile ${index >= 0 ? index + 1 : 1}`,
    });

  const uniqueProfileId = () => {
    const used = new Set(profiles.map((profile) => profile.id));
    let candidate = `profile-${Date.now().toString(36)}`;
    let suffix = 2;
    while (used.has(candidate)) {
      candidate = `profile-${Date.now().toString(36)}-${suffix}`;
      suffix += 1;
    }
    return candidate;
  };

  const activateProfile = (profileId: string) => {
    onChange({
      ...normalizedConfig,
      activeProfileByApp: {
        ...normalizedConfig.activeProfileByApp,
        [appId]: profileId,
      },
    });
  };

  const createProfile = () => {
    const id = uniqueProfileId();
    const name = t("home.modelTierRouting.profileNewName", {
      index: profiles.length + 1,
      defaultValue: `Profile ${profiles.length + 1}`,
    });
    onChange({
      ...normalizedConfig,
      profiles: [...profiles, { id, name, routes: {} }],
    });
    setEditingProfileId(id);
  };

  const duplicateProfile = (profile: ModelTierRoutingProfile) => {
    const id = uniqueProfileId();
    const name = t("home.modelTierRouting.profileCopyName", {
      name: profileDisplayName(profile),
      defaultValue: `${profileDisplayName(profile)} Copy`,
    });
    onChange({
      ...normalizedConfig,
      profiles: [
        ...profiles,
        {
          id,
          name,
          routes: JSON.parse(JSON.stringify(profile.routes ?? {})),
        },
      ],
    });
    setEditingProfileId(id);
  };

  const renameProfile = (profileId: string, name: string) => {
    onChange({
      ...normalizedConfig,
      profiles: profiles.map((profile) =>
        profile.id === profileId ? { ...profile, name } : profile,
      ),
    });
  };

  const deleteProfile = (profile: ModelTierRoutingProfile) => {
    if (profiles.length <= 1) return;
    setPendingDelete(profile);
  };

  const confirmDeleteProfile = () => {
    const profile = pendingDelete;
    setPendingDelete(null);
    if (!profile) return;

    const remaining = profiles.filter((item) => item.id !== profile.id);
    const fallbackId = remaining[0]?.id;
    if (!fallbackId) return;
    const activeProfileByApp = {
      ...(normalizedConfig.activeProfileByApp ?? {}),
    };
    for (const app of MODEL_TIER_ROUTING_APPS) {
      if (activeProfileByApp[app] === profile.id) {
        activeProfileByApp[app] = fallbackId;
      }
    }

    onChange({
      ...normalizedConfig,
      profiles: remaining,
      activeProfileByApp,
    });
    if (editingProfileId === profile.id) {
      setEditingProfileId(null);
    }
  };

  const handleTierChange = (
    profile: ModelTierRoutingProfile,
    tier: TierKey,
    patch: Partial<{
      providerId: string;
      model: string;
      displayName: string;
      supports1m: boolean;
    }>,
  ) => {
    onChange(
      writeRoute(normalizedConfig, appId, profile.id, tier, {
        ...readRoute(profile, appId, tier),
        ...patch,
      }),
    );
  };

  const editorView = editingProfile ? (
    <div className="space-y-3">
      <div className="rounded-xl glass-card p-5 space-y-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex min-w-0 items-center gap-3">
            <Button
              type="button"
              variant="ghost"
              size="icon"
              title={t("home.modelTierRouting.backToProfiles", {
                defaultValue: "Back to profiles",
              })}
              onClick={() => setEditingProfileId(null)}
            >
              <ArrowLeft className="h-4 w-4" />
            </Button>
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <h3 className="truncate text-base font-semibold">
                  {profileDisplayName(editingProfile)}
                </h3>
                {editingProfile.id === activeProfile.id && (
                  <span className="inline-flex shrink-0 items-center rounded-md bg-emerald-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-emerald-700 dark:text-emerald-300">
                    {t("home.modelTierRouting.activeProfile", {
                      defaultValue: "Active",
                    })}
                  </span>
                )}
              </div>
              <p className="text-sm text-muted-foreground">
                {t("home.modelTierRouting.editorDescription")}
              </p>
            </div>
          </div>
          <div className="flex gap-2">
            {editingProfile.id !== activeProfile.id && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => activateProfile(editingProfile.id)}
              >
                <CheckCircle2 className="h-4 w-4" />
                {t("home.modelTierRouting.useProfile", {
                  defaultValue: "Use",
                })}
              </Button>
            )}
            <Button
              type="button"
              variant="outline"
              size="icon"
              title={t("home.modelTierRouting.copyProfile", {
                defaultValue: "Copy profile",
              })}
              onClick={() => duplicateProfile(editingProfile)}
            >
              <Copy className="h-4 w-4" />
            </Button>
            <Button
              type="button"
              variant="outline"
              size="icon"
              title={t("home.modelTierRouting.deleteProfile", {
                defaultValue: "Delete profile",
              })}
              disabled={profiles.length <= 1}
              onClick={() => deleteProfile(editingProfile)}
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          </div>
        </div>

        <div className="space-y-1.5">
          <label className="px-1 text-xs font-medium text-muted-foreground">
            {t("home.modelTierRouting.profileName", {
              defaultValue: "Profile Name",
            })}
          </label>
          <Input
            className="min-w-0"
            value={editingProfile.name}
            onChange={(e) => renameProfile(editingProfile.id, e.target.value)}
          />
        </div>

        <div className="hidden sm:grid grid-cols-[5rem_1fr_1fr_1fr_116px] gap-3 px-1 text-xs font-medium text-muted-foreground">
          <span>{t("home.modelTierRouting.tier")}</span>
          <span>{t("home.modelTierRouting.provider")}</span>
          <span>{t("home.modelTierRouting.modelName")}</span>
          <span>{t("home.modelTierRouting.displayName")}</span>
          <span>
            {t("claudeDesktop.supports1mLabel", {
              defaultValue: "声明支持 1M",
            })}
          </span>
        </div>

        {TIERS.map((tier) => {
          const route = readRoute(editingProfile, appId, tier);
          const selectedProvider = route.providerId
            ? providerList.find((p) => p.id === route.providerId)
            : undefined;
          const isSelectedNonRoutable =
            !!selectedProvider && !supportsRouting(selectedProvider);
          return (
            <div
              key={tier}
              className="grid grid-cols-1 sm:grid-cols-[5rem_1fr_1fr_1fr_116px] gap-2 sm:gap-3 items-center"
            >
              <span className="capitalize text-sm font-medium px-1">
                {t(`settings.advanced.modelTierRouting.tier.${tier}`)}
              </span>
              <Select
                value={route.providerId || NONE_PROVIDER}
                onValueChange={(v) =>
                  handleTierChange(editingProfile, tier, {
                    providerId: v === NONE_PROVIDER ? "" : v,
                  })
                }
              >
                <SelectTrigger className="h-9 w-full min-w-0 text-sm font-normal">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NONE_PROVIDER}>
                    {t("settings.advanced.modelTierRouting.noProvider")}
                  </SelectItem>
                  {isSelectedNonRoutable && selectedProvider && (
                    <SelectItem
                      key={selectedProvider.id}
                      value={selectedProvider.id}
                      disabled
                    >
                      {selectedProvider.name}（
                      {t("claudeCode.noRoutingSupport")}）
                    </SelectItem>
                  )}
                  {routableProviders.map((p) => (
                    <SelectItem key={p.id} value={p.id}>
                      {p.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Input
                className="min-w-0"
                value={route.model}
                disabled={!route.providerId}
                placeholder={t(
                  "settings.advanced.modelTierRouting.modelPlaceholder",
                )}
                onChange={(e) =>
                  handleTierChange(editingProfile, tier, {
                    model: e.target.value,
                  })
                }
              />
              <Input
                className="min-w-0"
                value={route.displayName}
                disabled={!route.providerId}
                placeholder={t("home.modelTierRouting.displayNamePlaceholder")}
                onChange={(e) =>
                  handleTierChange(editingProfile, tier, {
                    displayName: e.target.value,
                  })
                }
              />
              <label className="flex h-9 items-center gap-2 text-sm text-muted-foreground">
                <Checkbox
                  checked={route.supports1m}
                  disabled={!route.providerId}
                  onCheckedChange={(checked) =>
                    handleTierChange(editingProfile, tier, {
                      supports1m: checked === true,
                    })
                  }
                />
                {t("claudeDesktop.supports1mShort", { defaultValue: "1M" })}
              </label>
            </div>
          );
        })}

        <p className="text-xs text-muted-foreground">
          {t("settings.advanced.modelTierRouting.hint")}
        </p>
      </div>
    </div>
  ) : null;

  const listView = !editingProfile ? (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3 px-1">
        <div className="min-w-0">
          <h3 className="text-base font-semibold">
            {t("settings.advanced.modelTierRouting.title")}
          </h3>
          <p className="text-sm text-muted-foreground">
            {t("home.modelTierRouting.listDescription", {
              defaultValue:
                "Click a profile to switch routing. Use the edit button to change tier mappings.",
            })}
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={createProfile}
        >
          <Plus className="h-4 w-4" />
          {t("home.modelTierRouting.addProfile", {
            defaultValue: "Add profile",
          })}
        </Button>
      </div>

      <div className="space-y-3">
        {profiles.map((profile) => {
          const isActive = profile.id === activeProfile.id;
          const configuredCount = TIERS.filter((tier) => {
            const route = readRoute(profile, appId, tier);
            return !!route.providerId && !!route.model.trim();
          }).length;
          return (
            <div
              key={profile.id}
              role="button"
              tabIndex={0}
              onClick={() => {
                if (!isActive) activateProfile(profile.id);
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  if (!isActive) activateProfile(profile.id);
                }
              }}
              className={cn(
                "cursor-pointer rounded-xl glass-card p-4 transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                isActive && "glass-card-active",
              )}
            >
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="min-w-0 space-y-1">
                  <div className="flex items-center gap-2">
                    <h4 className="truncate text-sm font-semibold">
                      {profileDisplayName(profile)}
                    </h4>
                    {isActive && (
                      <span className="inline-flex shrink-0 items-center rounded-md bg-emerald-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-emerald-700 dark:text-emerald-300">
                        {t("home.modelTierRouting.activeProfile", {
                          defaultValue: "Active",
                        })}
                      </span>
                    )}
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {t("home.modelTierRouting.configuredTiers", {
                      count: configuredCount,
                      total: TIERS.length,
                      defaultValue: "{{count}}/{{total}} tiers configured",
                    })}
                  </div>
                </div>
                <div
                  className="flex gap-2"
                  onClick={(e) => e.stopPropagation()}
                >
                  <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    title={t("home.modelTierRouting.editProfile", {
                      defaultValue: "Edit profile",
                    })}
                    onClick={() => setEditingProfileId(profile.id)}
                  >
                    <Pencil className="h-4 w-4" />
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    title={t("home.modelTierRouting.copyProfile", {
                      defaultValue: "Copy profile",
                    })}
                    onClick={() => duplicateProfile(profile)}
                  >
                    <Copy className="h-4 w-4" />
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    title={t("home.modelTierRouting.deleteProfile", {
                      defaultValue: "Delete profile",
                    })}
                    disabled={profiles.length <= 1}
                    onClick={() => deleteProfile(profile)}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              </div>

              <div className="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2">
                {TIERS.map((tier) => {
                  const route = readRoute(profile, appId, tier);
                  const providerName = route.providerId
                    ? (providerList.find((p) => p.id === route.providerId)
                        ?.name ?? route.providerId)
                    : t("settings.advanced.modelTierRouting.noProvider");
                  const isConfigured =
                    !!route.providerId && !!route.model.trim();
                  return (
                    <div
                      key={tier}
                      className={cn(
                        "min-w-0 rounded-lg border px-3 py-2 text-xs",
                        isConfigured
                          ? "border-border bg-background/40"
                          : "border-dashed border-muted-foreground/30 text-muted-foreground",
                      )}
                    >
                      <div className="flex items-center justify-between gap-2">
                        <span className="font-medium text-foreground">
                          {t(`settings.advanced.modelTierRouting.tier.${tier}`)}
                        </span>
                        {route.supports1m && (
                          <span className="rounded bg-blue-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-blue-700 dark:text-blue-300">
                            {t("claudeDesktop.supports1mShort", {
                              defaultValue: "1M",
                            })}
                          </span>
                        )}
                      </div>
                      <div className="mt-1 truncate">{providerName}</div>
                      {isConfigured && (
                        <div className="mt-0.5 truncate text-muted-foreground">
                          {route.model}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  ) : null;

  return (
    <>
      {editorView}
      {listView}
      <ConfirmDialog
        isOpen={Boolean(pendingDelete)}
        title={t("home.modelTierRouting.deleteProfileTitle", {
          defaultValue: "Delete routing profile",
        })}
        message={t("home.modelTierRouting.deleteProfileConfirm", {
          name: pendingDelete ? profileDisplayName(pendingDelete) : "",
          defaultValue: "Delete routing profile?",
        })}
        confirmText={t("common.delete")}
        variant="destructive"
        onConfirm={confirmDeleteProfile}
        onCancel={() => setPendingDelete(null)}
      />
    </>
  );
}
