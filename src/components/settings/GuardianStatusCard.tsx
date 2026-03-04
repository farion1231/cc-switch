import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { Activity, Loader2, RefreshCw, RotateCcw } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  settingsApi,
  type GuardianMigrationStatus,
  type GuardianStatus,
  type LegacyStartupMigrationResult,
  type LegacyStartupRollbackResult,
} from "@/lib/api/settings";
import type { StartupItemsMode } from "@/types";

type ActionKey = "refresh" | "toggle" | "diagnostic" | "migrate" | "rollback";
type MigrationUiStatus = "pending" | "running" | "migrated" | "failed";

interface GuardianStatusCardProps {
  enabled: boolean;
  guardianIntervalSeconds?: number;
  legacyStartupMigrated?: boolean;
  startupItemsMode?: StartupItemsMode;
  onToggleEnabled: (enabled: boolean) => Promise<void> | void;
}

interface ErrorBreakdown {
  auth_401: number;
  quota_429: number;
  upstream_5xx: number;
}

interface MigrationFeedback {
  error: boolean;
  message: string | null;
  backupId: string | null;
}

const EMPTY_BREAKDOWN: ErrorBreakdown = {
  auth_401: 0,
  quota_429: 0,
  upstream_5xx: 0,
};

const asObject = (value: unknown): Record<string, unknown> =>
  value && typeof value === "object" ? (value as Record<string, unknown>) : {};

const readString = (
  source: Record<string, unknown>,
  keys: string[],
): string | null => {
  for (const key of keys) {
    const value = source[key];
    if (typeof value === "string" && value.trim()) {
      return value.trim();
    }
  }
  return null;
};

const readNumber = (
  source: Record<string, unknown>,
  keys: string[],
): number | null => {
  for (const key of keys) {
    const value = source[key];
    if (typeof value === "number" && Number.isFinite(value)) {
      return value;
    }
    if (typeof value === "string" && value.trim()) {
      const parsed = Number(value);
      if (Number.isFinite(parsed)) {
        return parsed;
      }
    }
  }
  return null;
};

const normalizeBackupId = (rawPath: string | null): string | null => {
  if (!rawPath) return null;
  const parts = rawPath.split(/[\\/]/);
  const last = parts[parts.length - 1] ?? "";
  const compact = last.trim();
  if (!compact) return null;
  if (compact.toLowerCase() === "manifest.json" && parts.length >= 2) {
    const parent = (parts[parts.length - 2] ?? "").trim();
    return parent || null;
  }
  return compact.replace(/\.[^.]+$/, "");
};

const resolveBackupId = (
  payload: LegacyStartupMigrationResult | LegacyStartupRollbackResult,
): string | null => {
  const obj = asObject(payload);
  const explicitId = readString(obj, ["backupId", "backup_id", "backupID"]);
  if (explicitId) return explicitId;
  return normalizeBackupId(readString(obj, ["backupPath", "backup_path"]));
};

const getProxyHealth = (status: GuardianStatus | null): boolean | null => {
  if (!status) return null;
  const statusObj = asObject(status);
  const direct = statusObj.proxyHealthy;
  if (typeof direct === "boolean") return direct;

  const checksObj = asObject(statusObj.checks);
  const proxyHealth = asObject(
    checksObj.proxyHealth ?? checksObj.proxy_health ?? checksObj.health,
  );
  const ok = proxyHealth.ok;
  return typeof ok === "boolean" ? ok : null;
};

const getLastSelfHealAt = (status: GuardianStatus | null): string | null => {
  if (!status) return null;
  const statusObj = asObject(status);
  return (
    readString(statusObj, ["lastSelfHealAt", "last_self_heal_at", "lastSuccessAt"]) ??
    null
  );
};

const extractBreakdown = (status: GuardianStatus | null): ErrorBreakdown => {
  if (!status) return EMPTY_BREAKDOWN;
  const statusObj = asObject(status);
  const checksObj = asObject(statusObj.checks);
  const proxyDetails = asObject(asObject(checksObj.proxyHealth).details);
  const authDetails = asObject(asObject(checksObj.authNormalize).details);
  const breakerDetails = asObject(asObject(checksObj.breakerRecovery).details);

  const candidates = [
    asObject(statusObj.errors),
    asObject(statusObj.errorBreakdown),
    asObject(proxyDetails.errorBreakdown),
    asObject(authDetails.errorBreakdown),
    asObject(breakerDetails.errorBreakdown),
    asObject(statusObj.details),
  ];

  const pick = (keys: string[]): number => {
    for (const source of candidates) {
      const value = readNumber(source, keys);
      if (value !== null) return Math.max(0, value);
    }
    return 0;
  };

  const auth_401 = pick(["auth_401", "auth401", "auth401Count", "auth"]);
  const quota_429 = pick(["quota_429", "quota429", "quota429Count", "quota"]);
  const upstream_5xx = pick([
    "upstream_5xx",
    "upstream5xx",
    "upstream5xxCount",
    "upstream",
  ]);

  if (auth_401 || quota_429 || upstream_5xx) {
    return { auth_401, quota_429, upstream_5xx };
  }

  const lastError = readString(statusObj, ["lastError", "last_error"]) ?? "";
  if (!lastError) return EMPTY_BREAKDOWN;

  return {
    auth_401: /\b401\b/.test(lastError) ? 1 : 0,
    quota_429: /\b429\b/.test(lastError) ? 1 : 0,
    upstream_5xx: /\b5\d\d\b/.test(lastError) ? 1 : 0,
  };
};

export function GuardianStatusCard({
  enabled,
  guardianIntervalSeconds,
  legacyStartupMigrated,
  startupItemsMode,
  onToggleEnabled,
}: GuardianStatusCardProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [status, setStatus] = useState<GuardianStatus | null>(null);
  const [guardianMigration, setGuardianMigration] =
    useState<GuardianMigrationStatus | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [pendingAction, setPendingAction] = useState<ActionKey | null>(null);
  const [migrationFeedback, setMigrationFeedback] = useState<MigrationFeedback>({
    error: false,
    message: null,
    backupId: null,
  });

  const fetchStatus = useCallback(async () => {
    const next = await settingsApi.getGuardianStatus();
    setStatus(next);
    return next;
  }, []);

  const fetchMigrationStatus = useCallback(async () => {
    try {
      const next = await settingsApi.getGuardianMigrationStatus();
      setGuardianMigration(next);
      return next;
    } catch (error) {
      console.warn("Failed to load guardian migration status:", error);
      return null;
    }
  }, []);

  const refreshAll = useCallback(
    async (showToast = false, background = false) => {
      if (!background) {
        setPendingAction("refresh");
      }
      try {
        await Promise.all([
          fetchStatus(),
          fetchMigrationStatus(),
          queryClient.invalidateQueries({ queryKey: ["settings"] }),
        ]);
        if (showToast) {
          toast.success(
            t("settings.advanced.guardian.refreshDone", {
              defaultValue: "Guardian status refreshed",
            }),
          );
        }
      } catch (error) {
        console.error("Failed to refresh guardian:", error);
        toast.error(
          t("settings.advanced.guardian.refreshFailed", {
            defaultValue: "Failed to refresh Guardian status",
          }),
        );
      } finally {
        if (!background) {
          setPendingAction((prev) => (prev === "refresh" ? null : prev));
        }
      }
    },
    [fetchMigrationStatus, fetchStatus, queryClient, t],
  );

  useEffect(() => {
    void (async () => {
      try {
        await Promise.all([fetchStatus(), fetchMigrationStatus()]);
      } catch (error) {
        console.error("Failed to load guardian status:", error);
        toast.error(
          t("settings.advanced.guardian.loadFailed", {
            defaultValue: "Failed to load Guardian status",
          }),
        );
      } finally {
        setIsLoading(false);
      }
    })();
  }, [fetchMigrationStatus, fetchStatus, t]);

  const proxyHealthy = useMemo(() => getProxyHealth(status), [status]);
  const lastSelfHealAt = useMemo(() => getLastSelfHealAt(status), [status]);
  const errorBreakdown = useMemo(() => extractBreakdown(status), [status]);

  const lastSelfHealDisplay = useMemo(() => {
    if (!lastSelfHealAt) {
      return t("settings.advanced.guardian.never", { defaultValue: "Never" });
    }
    const d = new Date(lastSelfHealAt);
    return Number.isNaN(d.getTime()) ? lastSelfHealAt : d.toLocaleString();
  }, [lastSelfHealAt, t]);

  const guardianMigrationObj = useMemo(
    () => asObject(guardianMigration),
    [guardianMigration],
  );

  const remoteMigrationStatus = useMemo(
    () =>
      (readString(guardianMigrationObj, ["status", "state"]) ?? "").toLowerCase(),
    [guardianMigrationObj],
  );

  const remoteMigrationMessage = useMemo(
    () => readString(guardianMigrationObj, ["message"]),
    [guardianMigrationObj],
  );

  const remoteBackupId = useMemo(
    () => readString(guardianMigrationObj, ["backupId", "backup_id"]),
    [guardianMigrationObj],
  );

  const migrationStatus = useMemo<MigrationUiStatus>(() => {
    if (pendingAction === "migrate" || pendingAction === "rollback") {
      return "running";
    }
    if (migrationFeedback.error) return "failed";
    if (remoteMigrationStatus === "migrated") return "migrated";
    if (remoteMigrationStatus === "pending") return "pending";
    if (remoteMigrationStatus === "running") return "running";
    if (
      remoteMigrationStatus === "needs_attention" ||
      remoteMigrationStatus === "failed" ||
      remoteMigrationStatus === "error"
    ) {
      return "failed";
    }
    return legacyStartupMigrated ? "migrated" : "pending";
  }, [
    legacyStartupMigrated,
    migrationFeedback.error,
    pendingAction,
    remoteMigrationStatus,
  ]);

  const startupModeLabel = useMemo(() => {
    const mode = startupItemsMode ?? "autoLaunch";
    return t(`settings.advanced.guardian.startupModeValues.${mode}`, {
      defaultValue: mode,
    });
  }, [startupItemsMode, t]);

  const runDiagnostic = useCallback(async () => {
    setPendingAction("diagnostic");
    try {
      const result = await settingsApi.runGuardianDiagnostic();
      setStatus(result);
      toast.success(
        t("settings.advanced.guardian.diagnosticSuccess", {
          defaultValue: "Diagnostics completed",
        }),
      );
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    } catch (error) {
      console.error("Failed to run guardian diagnostic:", error);
      toast.error(
        t("settings.advanced.guardian.diagnosticFailed", {
          defaultValue: "Diagnostics failed",
        }),
      );
    } finally {
      setPendingAction((prev) => (prev === "diagnostic" ? null : prev));
    }
  }, [queryClient, t]);

  const applyMigrationFeedback = useCallback(
    (
      payload: LegacyStartupMigrationResult | LegacyStartupRollbackResult,
      message: string,
      error: boolean,
    ) => {
      setMigrationFeedback({
        error,
        message,
        backupId: resolveBackupId(payload),
      });
    },
    [],
  );

  const migrateLegacyItems = useCallback(async () => {
    setPendingAction("migrate");
    try {
      const result = await settingsApi.migrateLegacyStartupItems();
      const skipped = Boolean(result.skipped || result.alreadyMigrated);
      applyMigrationFeedback(
        result,
        skipped
          ? t("settings.advanced.guardian.migrateSkipped", {
              defaultValue: "Already migrated",
            })
          : t("settings.advanced.guardian.migrateSuccess", {
              defaultValue: "Legacy items migrated",
            }),
        false,
      );
      await refreshAll(false, true);
    } catch (error) {
      console.error("Failed to migrate legacy startup items:", error);
      const message =
        (error as Error)?.message ??
        t("settings.advanced.guardian.migrateFailed", {
          defaultValue: "Migration failed",
        });
      setMigrationFeedback({ error: true, message, backupId: null });
      toast.error(
        t("settings.advanced.guardian.migrateFailed", {
          defaultValue: "Migration failed",
        }),
      );
    } finally {
      setPendingAction((prev) => (prev === "migrate" ? null : prev));
    }
  }, [applyMigrationFeedback, refreshAll, t]);

  const rollbackLegacyItems = useCallback(async () => {
    setPendingAction("rollback");
    try {
      const targetBackupId = migrationFeedback.backupId ?? remoteBackupId;
      const result = targetBackupId
        ? await settingsApi.rollbackLegacyMigrationWithBackupId(targetBackupId)
        : await settingsApi.rollbackLegacyMigration();
      applyMigrationFeedback(
        result,
        t("settings.advanced.guardian.rollbackSuccess", {
          defaultValue: "Legacy rollback completed",
        }),
        false,
      );
      await refreshAll(false, true);
    } catch (error) {
      console.error("Failed to rollback legacy migration:", error);
      const message =
        (error as Error)?.message ??
        t("settings.advanced.guardian.rollbackFailed", {
          defaultValue: "Rollback failed",
        });
      setMigrationFeedback({ error: true, message, backupId: null });
      toast.error(
        t("settings.advanced.guardian.rollbackFailed", {
          defaultValue: "Rollback failed",
        }),
      );
    } finally {
      setPendingAction((prev) => (prev === "rollback" ? null : prev));
    }
  }, [applyMigrationFeedback, migrationFeedback.backupId, refreshAll, remoteBackupId, t]);

  const toggleEnabled = useCallback(
    async (checked: boolean) => {
      setPendingAction("toggle");
      try {
        const nextStatus = await settingsApi.setGuardianEnabled(checked);
        setStatus(nextStatus);
        await onToggleEnabled(checked);
        await queryClient.invalidateQueries({ queryKey: ["settings"] });
      } catch (error) {
        console.error("Failed to toggle guardian:", error);
        toast.error(
          t("settings.advanced.guardian.toggleFailed", {
            defaultValue: "Failed to update Guardian",
          }),
        );
      } finally {
        setPendingAction((prev) => (prev === "toggle" ? null : prev));
      }
    },
    [onToggleEnabled, queryClient, t],
  );

  const migrationMessage = migrationFeedback.message ?? remoteMigrationMessage;
  const backupIdDisplay = migrationFeedback.backupId ?? remoteBackupId;
  const intervalSecondsDisplay =
    guardianIntervalSeconds ?? status?.intervalSeconds ?? 60;

  if (isLoading) {
    return (
      <div className="flex justify-center py-4">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  const healthVariant: "default" | "secondary" | "destructive" =
    proxyHealthy === true
      ? "default"
      : proxyHealthy === false
        ? "destructive"
        : "secondary";

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="space-y-0.5">
          <Label>{t("settings.advanced.guardian.enabled")}</Label>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.guardian.enabledDescription")}
          </p>
        </div>
        <Switch
          checked={enabled}
          disabled={pendingAction === "toggle"}
          onCheckedChange={(checked) => void toggleEnabled(checked)}
        />
      </div>

      <div className="grid gap-3 sm:grid-cols-3">
        <div className="rounded-lg border border-border/60 bg-muted/30 px-3 py-2 space-y-1">
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.guardian.proxyHealth")}
          </p>
          <Badge variant={healthVariant} className="w-fit">
            {proxyHealthy === true
              ? t("settings.advanced.guardian.health.healthy")
              : proxyHealthy === false
                ? t("settings.advanced.guardian.health.unhealthy")
                : t("settings.advanced.guardian.health.unknown")}
          </Badge>
        </div>
        <div className="rounded-lg border border-border/60 bg-muted/30 px-3 py-2 space-y-1">
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.guardian.lastSelfHealAt")}
          </p>
          <p className="text-sm font-medium">{lastSelfHealDisplay}</p>
        </div>
        <div className="rounded-lg border border-border/60 bg-muted/30 px-3 py-2 space-y-1">
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.guardian.intervalSeconds")}
          </p>
          <p className="text-sm font-medium">{intervalSecondsDisplay}s</p>
        </div>
      </div>

      <div className="space-y-2">
        <p className="text-xs text-muted-foreground">
          {t("settings.advanced.guardian.errorBreakdown")}
        </p>
        <div className="grid gap-2 sm:grid-cols-3">
          <div className="rounded-lg border border-border/60 bg-muted/30 px-3 py-2">
            <p className="text-xs text-muted-foreground">auth_401</p>
            <p className="text-sm font-medium">{errorBreakdown.auth_401}</p>
          </div>
          <div className="rounded-lg border border-border/60 bg-muted/30 px-3 py-2">
            <p className="text-xs text-muted-foreground">quota_429</p>
            <p className="text-sm font-medium">{errorBreakdown.quota_429}</p>
          </div>
          <div className="rounded-lg border border-border/60 bg-muted/30 px-3 py-2">
            <p className="text-xs text-muted-foreground">upstream_5xx</p>
            <p className="text-sm font-medium">{errorBreakdown.upstream_5xx}</p>
          </div>
        </div>
      </div>

      <div className="rounded-lg border border-border/60 bg-muted/20 px-3 py-2.5 space-y-2">
        <div className="flex flex-wrap items-center gap-2 text-sm">
          <span className="text-muted-foreground">
            {t("settings.advanced.guardian.migrationStatus")}:
          </span>
          <Badge
            variant={
              migrationStatus === "failed"
                ? "destructive"
                : migrationStatus === "migrated"
                  ? "default"
                  : "secondary"
            }
          >
            {t(`settings.advanced.guardian.migration.${migrationStatus}`)}
          </Badge>
          <span className="text-xs text-muted-foreground">
            {t("settings.advanced.guardian.startupMode")}: {startupModeLabel}
          </span>
        </div>
        {migrationMessage ? (
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.guardian.lastMigrationMessage")}:{" "}
            {migrationMessage}
          </p>
        ) : null}
        {backupIdDisplay ? (
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.guardian.backupId")}:{" "}
            <span className="font-mono">{backupIdDisplay}</span>
          </p>
        ) : null}
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Button
          variant="outline"
          disabled={pendingAction === "refresh"}
          onClick={() => void refreshAll(true)}
        >
          {pendingAction === "refresh" ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <RefreshCw className="mr-2 h-4 w-4" />
          )}
          {pendingAction === "refresh"
            ? t("settings.advanced.guardian.refreshing")
            : t("settings.advanced.guardian.refreshAll")}
        </Button>

        <Button
          variant="outline"
          disabled={pendingAction === "diagnostic"}
          onClick={() => void runDiagnostic()}
        >
          {pendingAction === "diagnostic" ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <Activity className="mr-2 h-4 w-4" />
          )}
          {pendingAction === "diagnostic"
            ? t("settings.advanced.guardian.runningDiagnostic")
            : t("settings.advanced.guardian.runDiagnostic")}
        </Button>

        <Button
          variant="outline"
          disabled={pendingAction === "migrate"}
          onClick={() => void migrateLegacyItems()}
        >
          {pendingAction === "migrate" ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <RefreshCw className="mr-2 h-4 w-4" />
          )}
          {pendingAction === "migrate"
            ? t("settings.advanced.guardian.migratingLegacy")
            : t("settings.advanced.guardian.migrateLegacy")}
        </Button>

        <Button
          variant="outline"
          disabled={pendingAction === "rollback"}
          onClick={() => void rollbackLegacyItems()}
        >
          {pendingAction === "rollback" ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <RotateCcw className="mr-2 h-4 w-4" />
          )}
          {pendingAction === "rollback"
            ? t("settings.advanced.guardian.rollingBackLegacy")
            : t("settings.advanced.guardian.rollbackLegacy")}
        </Button>
      </div>
    </div>
  );
}
