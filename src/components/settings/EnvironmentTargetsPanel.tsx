import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertCircle,
  CheckCircle2,
  History,
  Loader2,
  RefreshCw,
  RotateCcw,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { providersApi, settingsApi } from "@/lib/api";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { isWindows } from "@/lib/platform";
import type {
  ManagedTarget,
  TargetArtifactState,
  TargetInspection,
  WslTargetDiscovery,
} from "@/types";

interface TargetView {
  target: ManagedTarget;
  inspection?: TargetInspection;
  inspectionFailed?: boolean;
}

interface HistoryAction {
  target: ManagedTarget;
  type: "migrate" | "restore";
}

const NO_PROVIDER_LINK = "__cc_switch_no_provider_link__";

function artifactClass(state: TargetArtifactState): string {
  if (state === "valid") return "border-emerald-500/40 text-emerald-600";
  if (state === "invalid") return "border-red-500/40 text-red-600";
  return "border-amber-500/40 text-amber-600";
}

function historySkippedMessageKey(
  actionType: HistoryAction["type"],
  skippedReason: string,
): string {
  switch (skippedReason) {
    case "live_not_unified":
      return "settings.environments.historyLiveNotUnified";
    case "already_unified":
      return "settings.environments.historyAlreadyUnified";
    case "no_backup_ledger":
      return "settings.environments.historyNoBackupLedger";
    case "nothing_to_restore":
      return "settings.environments.historyNothingToRestore";
    default:
      return actionType === "migrate"
        ? "settings.environments.historyAlreadyUnified"
        : "settings.environments.historyNothingToRestore";
  }
}

export function EnvironmentTargetsPanel() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { takeoverStatus } = useProxyStatus();
  const isCodexTakeoverActive = takeoverStatus?.codex ?? false;
  const [activationTarget, setActivationTarget] =
    useState<ManagedTarget | null>(null);
  const [historyAction, setHistoryAction] = useState<HistoryAction | null>(
    null,
  );
  const query = useQuery({
    queryKey: ["managed-targets", "inspection"],
    queryFn: async (): Promise<TargetView[]> => {
      const targets = await settingsApi.listManagedTargets();
      return await Promise.all(
        targets.map(async (target) => {
          try {
            const inspection = await settingsApi.inspectManagedTarget(
              target.id,
            );
            return { target, inspection };
          } catch {
            return { target, inspectionFailed: true };
          }
        }),
      );
    },
  });
  const discovery = useQuery({
    queryKey: ["managed-targets", "wsl-discovery"],
    queryFn: (): Promise<WslTargetDiscovery[]> =>
      settingsApi.discoverWslTargets(),
    enabled: false,
    retry: false,
  });
  const providers = useQuery({
    queryKey: ["managed-targets", "provider-options", "codex"],
    queryFn: () => providersApi.getAll("codex"),
  });
  const registration = useMutation({
    mutationFn: (distro: string) =>
      settingsApi.registerDiscoveredWslTarget(distro),
    onSuccess: async () => {
      toast.success(t("settings.environments.registerSuccess"));
      await queryClient.invalidateQueries({ queryKey: ["managed-targets"] });
    },
    onError: () => {
      toast.error(t("settings.environments.registerFailed"));
    },
  });
  const providerLink = useMutation({
    mutationFn: ({
      targetId,
      providerId,
    }: {
      targetId: string;
      providerId: string | null;
    }) => settingsApi.linkManagedTargetProvider(targetId, providerId),
    onSuccess: async () => {
      toast.success(t("settings.environments.linkSuccess"));
      await queryClient.invalidateQueries({ queryKey: ["managed-targets"] });
    },
    onError: () => {
      toast.error(t("settings.environments.linkFailed"));
    },
  });
  const activation = useMutation({
    mutationFn: (targetId: string) =>
      settingsApi.activateWslManagedTarget(targetId),
    onSuccess: async () => {
      setActivationTarget(null);
      toast.success(t("settings.environments.activationSuccess"));
      await queryClient.invalidateQueries({ queryKey: ["managed-targets"] });
    },
    onError: () => {
      toast.error(t("settings.environments.activationFailed"));
    },
  });
  const targetSwitch = useMutation({
    mutationFn: ({
      targetId,
      providerId,
    }: {
      targetId: string;
      providerId: string;
    }) => settingsApi.switchManagedTargetProvider(targetId, providerId),
    onSuccess: async () => {
      toast.success(t("settings.environments.switchSuccess"));
      await queryClient.invalidateQueries({ queryKey: ["managed-targets"] });
    },
    onError: () => {
      toast.error(t("settings.environments.switchFailed"));
    },
  });
  const historyMutation = useMutation({
    mutationFn: async (action: HistoryAction) => {
      const result =
        action.type === "migrate"
          ? await settingsApi.migrateManagedTargetCodexHistory(action.target.id)
          : await settingsApi.restoreManagedTargetCodexHistory(
              action.target.id,
            );
      return { action, result };
    },
    onSuccess: async ({ action, result }) => {
      setHistoryAction(null);
      if (result.skippedReason) {
        toast.info(
          t(historySkippedMessageKey(action.type, result.skippedReason)),
        );
      } else {
        toast.success(
          t(
            action.type === "migrate"
              ? "settings.environments.historyMigrationSuccess"
              : "settings.environments.historyRestoreSuccess",
            {
              files: result.changedJsonlFiles,
              rows: result.changedStateRows,
            },
          ),
        );
      }
      await queryClient.invalidateQueries({ queryKey: ["managed-targets"] });
    },
    onError: (_error, action) => {
      toast.error(
        t(
          action.type === "migrate"
            ? "settings.environments.historyMigrationFailed"
            : "settings.environments.historyRestoreFailed",
        ),
      );
    },
  });

  const artifactLabel = (state: TargetArtifactState) =>
    t(`settings.environments.artifact.${state}`);

  const activationProviderId = activationTarget?.currentProviderId ?? "";
  const activationProviderLabel =
    (activationProviderId && providers.data?.[activationProviderId]?.name) ||
    activationProviderId ||
    t("settings.environments.selectProvider");

  return (
    <div className="space-y-4">
      <div className="flex items-start justify-between gap-4">
        <p className="text-sm text-muted-foreground">
          {t("settings.environments.readOnlyNotice")}
        </p>
        <div className="flex shrink-0 flex-wrap justify-end gap-2">
          {isWindows() ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={discovery.isFetching}
              onClick={() => void discovery.refetch()}
            >
              <RefreshCw
                className={`h-4 w-4 ${discovery.isFetching ? "animate-spin" : ""}`}
              />
              {t("settings.environments.discoverWsl")}
            </Button>
          ) : null}
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={query.isFetching}
            onClick={() => void query.refetch()}
          >
            <RefreshCw
              className={`h-4 w-4 ${query.isFetching ? "animate-spin" : ""}`}
            />
            {t("settings.environments.refresh")}
          </Button>
        </div>
      </div>

      {query.isLoading ? (
        <div className="flex items-center justify-center py-10 text-muted-foreground">
          <Loader2 className="mr-2 h-5 w-5 animate-spin" />
          {t("settings.environments.loading")}
        </div>
      ) : query.isError ? (
        <div className="flex items-center gap-2 rounded-lg border border-red-500/30 bg-red-500/5 p-4 text-sm text-red-600">
          <AlertCircle className="h-4 w-4 shrink-0" />
          {t("settings.environments.loadFailed")}
        </div>
      ) : query.data?.length ? (
        <div className="space-y-3">
          {query.data.map(({ target, inspection, inspectionFailed }) => (
            <Card key={target.id} className="shadow-none">
              <CardHeader className="space-y-3 p-4 pb-3">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <CardTitle className="truncate text-base">
                      {target.name}
                    </CardTitle>
                    <p className="mt-1 break-all font-mono text-xs text-muted-foreground">
                      {target.configLocation.path}
                    </p>
                  </div>
                  <Badge
                    variant="outline"
                    className={
                      inspection?.reachable
                        ? "border-emerald-500/40 text-emerald-600"
                        : "border-red-500/40 text-red-600"
                    }
                  >
                    {inspection?.reachable
                      ? t("settings.environments.reachable")
                      : t("settings.environments.unreachable")}
                  </Badge>
                </div>
                <div className="flex flex-wrap gap-2 text-xs">
                  <Badge variant="secondary">
                    {target.kind.type === "wsl"
                      ? `WSL · ${target.kind.distro} · ${target.kind.user}`
                      : t("settings.environments.windows")}
                  </Badge>
                  <Badge variant="secondary">
                    {t("settings.environments.currentProvider")}:{" "}
                    {target.currentProviderId ??
                      t("settings.environments.notLinked")}
                  </Badge>
                </div>
              </CardHeader>
              <CardContent className="p-4 pt-0">
                {inspection ? (
                  <div className="grid gap-2 text-sm sm:grid-cols-2">
                    <div className="flex items-center justify-between rounded-md bg-muted/40 px-3 py-2">
                      <span>config.toml</span>
                      <Badge
                        variant="outline"
                        className={artifactClass(inspection.config)}
                      >
                        {artifactLabel(inspection.config)}
                      </Badge>
                    </div>
                    <div className="flex items-center justify-between rounded-md bg-muted/40 px-3 py-2">
                      <span>auth.json</span>
                      <Badge
                        variant="outline"
                        className={artifactClass(inspection.auth)}
                      >
                        {artifactLabel(inspection.auth)}
                      </Badge>
                    </div>
                    <div className="rounded-md bg-muted/40 px-3 py-2">
                      {t("settings.environments.sessions", {
                        active: inspection.activeSessionCount,
                        archived: inspection.archivedSessionCount,
                      })}
                    </div>
                    <div className="flex items-center gap-2 rounded-md bg-muted/40 px-3 py-2">
                      {inspection.stateDbPresent ? (
                        <CheckCircle2 className="h-4 w-4 text-emerald-600" />
                      ) : (
                        <AlertCircle className="h-4 w-4 text-amber-600" />
                      )}
                      {inspection.stateDbPresent
                        ? t("settings.environments.stateDbPresent")
                        : t("settings.environments.stateDbMissing")}
                    </div>
                  </div>
                ) : (
                  <div className="flex items-start gap-2 rounded-md bg-red-500/5 px-3 py-2 text-sm text-red-600">
                    <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
                    <span>
                      {inspectionFailed
                        ? t("settings.environments.inspectionFailed")
                        : t("settings.environments.unreachable")}
                    </span>
                  </div>
                )}
                {target.managementState === "unmanaged" ? (
                  <div className="mt-3 space-y-2 border-t pt-3">
                    <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                      <div>
                        <p className="text-sm font-medium">
                          {t("settings.environments.providerLink")}
                        </p>
                        <p className="text-xs text-muted-foreground">
                          {t("settings.environments.providerLinkNotice")}
                        </p>
                      </div>
                      <Select
                        value={target.currentProviderId ?? NO_PROVIDER_LINK}
                        disabled={providers.isLoading || providerLink.isPending}
                        onValueChange={(value) =>
                          providerLink.mutate({
                            targetId: target.id,
                            providerId:
                              value === NO_PROVIDER_LINK ? null : value,
                          })
                        }
                      >
                        <SelectTrigger className="w-full sm:w-64">
                          <SelectValue
                            placeholder={t(
                              "settings.environments.selectProvider",
                            )}
                          />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value={NO_PROVIDER_LINK}>
                            {t("settings.environments.keepUnmanaged")}
                          </SelectItem>
                          {Object.values(providers.data ?? {}).map(
                            (provider) => (
                              <SelectItem key={provider.id} value={provider.id}>
                                {provider.name}
                              </SelectItem>
                            ),
                          )}
                        </SelectContent>
                      </Select>
                    </div>
                    {providers.isError ? (
                      <p className="text-xs text-red-600">
                        {t("settings.environments.providersLoadFailed")}
                      </p>
                    ) : null}
                    {target.kind.type === "wsl" && target.currentProviderId ? (
                      <div className="flex items-center justify-between gap-3 rounded-md border border-blue-500/20 bg-blue-500/5 p-3">
                        <p className="text-xs text-muted-foreground">
                          {t("settings.environments.activationNotice")}
                        </p>
                        <Button
                          type="button"
                          size="sm"
                          className="shrink-0"
                          disabled={
                            !inspection?.reachable || activation.isPending
                          }
                          onClick={() => setActivationTarget(target)}
                        >
                          {activation.isPending &&
                          activation.variables === target.id ? (
                            <Loader2 className="h-4 w-4 animate-spin" />
                          ) : null}
                          {t("settings.environments.activate")}
                        </Button>
                      </div>
                    ) : null}
                  </div>
                ) : null}
                {target.managementState === "managed" ? (
                  <div className="mt-3 border-t pt-3">
                    <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                      <div>
                        <p className="text-sm font-medium">
                          {t("settings.environments.switchProvider")}
                        </p>
                        <p className="text-xs text-muted-foreground">
                          {t("settings.environments.switchNotice")}
                        </p>
                      </div>
                      <Select
                        value={target.currentProviderId ?? undefined}
                        disabled={
                          providers.isLoading ||
                          targetSwitch.isPending ||
                          (isCodexTakeoverActive &&
                            target.kind.type === "localWindows")
                        }
                        onValueChange={(providerId) => {
                          if (providerId !== target.currentProviderId) {
                            targetSwitch.mutate({
                              targetId: target.id,
                              providerId,
                            });
                          }
                        }}
                      >
                        <SelectTrigger className="w-full sm:w-64">
                          <SelectValue
                            placeholder={t(
                              "settings.environments.selectProvider",
                            )}
                          />
                        </SelectTrigger>
                        <SelectContent>
                          {Object.values(providers.data ?? {}).map(
                            (provider) => (
                              <SelectItem key={provider.id} value={provider.id}>
                                {provider.name}
                              </SelectItem>
                            ),
                          )}
                        </SelectContent>
                      </Select>
                    </div>
                    {isCodexTakeoverActive &&
                    target.kind.type === "localWindows" ? (
                      <p className="mt-2 text-xs text-amber-600">
                        {t("settings.environments.proxyTakeoverUnsupported")}
                      </p>
                    ) : null}
                  </div>
                ) : null}
                {inspection?.reachable &&
                target.managementState === "managed" ? (
                  <div className="mt-3 border-t pt-3">
                    <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                      <div>
                        <p className="flex items-center gap-2 text-sm font-medium">
                          <History className="h-4 w-4" />
                          {t("settings.environments.historyCompatibility")}
                        </p>
                        <p className="mt-1 text-xs text-muted-foreground">
                          {t(
                            "settings.environments.historyCompatibilityNotice",
                          )}
                        </p>
                      </div>
                      <div className="flex shrink-0 flex-wrap gap-2">
                        <Button
                          type="button"
                          size="sm"
                          variant="outline"
                          disabled={historyMutation.isPending}
                          onClick={() =>
                            setHistoryAction({ target, type: "restore" })
                          }
                        >
                          {historyMutation.isPending &&
                          historyMutation.variables?.target.id === target.id &&
                          historyMutation.variables.type === "restore" ? (
                            <Loader2 className="h-4 w-4 animate-spin" />
                          ) : (
                            <RotateCcw className="h-4 w-4" />
                          )}
                          {t("settings.environments.restoreHistory")}
                        </Button>
                        <Button
                          type="button"
                          size="sm"
                          disabled={historyMutation.isPending}
                          onClick={() =>
                            setHistoryAction({ target, type: "migrate" })
                          }
                        >
                          {historyMutation.isPending &&
                          historyMutation.variables?.target.id === target.id &&
                          historyMutation.variables.type === "migrate" ? (
                            <Loader2 className="h-4 w-4 animate-spin" />
                          ) : (
                            <History className="h-4 w-4" />
                          )}
                          {t("settings.environments.migrateHistory")}
                        </Button>
                      </div>
                    </div>
                  </div>
                ) : null}
              </CardContent>
            </Card>
          ))}
        </div>
      ) : (
        <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
          {t("settings.environments.empty")}
        </div>
      )}

      {discovery.isError ? (
        <div className="flex items-center gap-2 rounded-lg border border-red-500/30 bg-red-500/5 p-4 text-sm text-red-600">
          <AlertCircle className="h-4 w-4 shrink-0" />
          {t("settings.environments.discoveryFailed")}
        </div>
      ) : discovery.data ? (
        <div className="space-y-3 border-t pt-4">
          <div>
            <h4 className="text-sm font-semibold">
              {t("settings.environments.discoveredTitle")}
            </h4>
            <p className="text-xs text-muted-foreground">
              {t("settings.environments.discoveredDescription")}
            </p>
          </div>
          {discovery.data.length ? (
            discovery.data.map((candidate) => {
              const registered = query.data?.some(
                ({ target }) =>
                  target.kind.type === "wsl" &&
                  target.kind.distro === candidate.distro &&
                  target.kind.user === candidate.user,
              );
              return (
                <div
                  key={`${candidate.distro}:${candidate.user ?? "offline"}`}
                  className="flex items-start justify-between gap-3 rounded-lg border p-3"
                >
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-medium">{candidate.distro}</span>
                      {candidate.user ? (
                        <Badge variant="secondary">{candidate.user}</Badge>
                      ) : null}
                      {registered ? (
                        <Badge variant="outline">
                          {t("settings.environments.registered")}
                        </Badge>
                      ) : null}
                    </div>
                    <p className="mt-1 break-all font-mono text-xs text-muted-foreground">
                      {candidate.configPath ??
                        t("settings.environments.homeUnavailable")}
                    </p>
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    <Badge
                      variant="outline"
                      className={
                        candidate.reachable
                          ? candidate.codexConfigPresent
                            ? "border-emerald-500/40 text-emerald-600"
                            : "border-amber-500/40 text-amber-600"
                          : "border-red-500/40 text-red-600"
                      }
                    >
                      {!candidate.reachable
                        ? t("settings.environments.offline")
                        : candidate.codexConfigPresent
                          ? t("settings.environments.codexFound")
                          : t("settings.environments.codexMissing")}
                    </Badge>
                    {!registered &&
                    candidate.reachable &&
                    candidate.codexConfigPresent ? (
                      <Button
                        type="button"
                        size="sm"
                        disabled={registration.isPending}
                        onClick={() => registration.mutate(candidate.distro)}
                      >
                        {registration.isPending &&
                        registration.variables === candidate.distro ? (
                          <Loader2 className="h-4 w-4 animate-spin" />
                        ) : null}
                        {t("settings.environments.register")}
                      </Button>
                    ) : null}
                  </div>
                </div>
              );
            })
          ) : (
            <div className="rounded-lg border border-dashed p-5 text-center text-sm text-muted-foreground">
              {t("settings.environments.noWslDistros")}
            </div>
          )}
        </div>
      ) : null}

      <ConfirmDialog
        isOpen={activationTarget !== null}
        variant="info"
        title={t("settings.environments.activationConfirmTitle")}
        message={t("settings.environments.activationConfirmMessage", {
          target: activationTarget?.name ?? "",
          provider: activationProviderLabel,
        })}
        confirmText={t("settings.environments.activate")}
        confirmDisabled={activation.isPending}
        onConfirm={() => {
          if (!activationTarget || activation.isPending) return;
          activation.mutate(activationTarget.id);
        }}
        onCancel={() => {
          if (!activation.isPending) setActivationTarget(null);
        }}
      />
      <ConfirmDialog
        isOpen={historyAction !== null}
        variant="info"
        title={t(
          historyAction?.type === "restore"
            ? "settings.environments.historyRestoreConfirmTitle"
            : "settings.environments.historyMigrationConfirmTitle",
        )}
        message={t(
          historyAction?.type === "restore"
            ? "settings.environments.historyRestoreConfirmMessage"
            : "settings.environments.historyMigrationConfirmMessage",
          { target: historyAction?.target.name ?? "" },
        )}
        confirmText={t(
          historyAction?.type === "restore"
            ? "settings.environments.confirmRestore"
            : "settings.environments.confirmMigration",
        )}
        confirmDisabled={historyMutation.isPending}
        onConfirm={() => {
          if (!historyAction || historyMutation.isPending) return;
          const action = historyAction;
          setHistoryAction(null);
          historyMutation.mutate(action);
        }}
        onCancel={() => {
          if (!historyMutation.isPending) setHistoryAction(null);
        }}
      />
    </div>
  );
}
