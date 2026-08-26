import { useState } from "react";
import {
  BadgeCheck,
  KeyRound,
  Loader2,
  Radio,
  ShieldAlert,
  ShieldCheck,
  ShieldMinus,
} from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Switch } from "@/components/ui/switch";
import {
  useCursorProviders,
  useCursorRuntimeState,
  useInstallCursorCA,
  useRemoveCursorCA,
  useStartCursorRuntime,
  useStopCursorRuntime,
} from "@/lib/query/cursor";
import type { CursorRuntimeState } from "@/lib/api/cursor";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";

interface CursorRuntimeToggleProps {
  className?: string;
}

const BUSY_PHASES = new Set(["starting", "restoring", "maintenance"]);

export function CursorRuntimeToggle({ className }: CursorRuntimeToggleProps) {
  const { t } = useTranslation();
  const providersQuery = useCursorProviders();
  const runtimeQuery = useCursorRuntimeState();
  const startRuntime = useStartCursorRuntime();
  const stopRuntime = useStopCursorRuntime();
  const installCA = useInstallCursorCA();
  const removeCA = useRemoveCursorCA();
  const [trustDialogOpen, setTrustDialogOpen] = useState(false);
  const [caDialogOpen, setCADialogOpen] = useState(false);

  const state = runtimeQuery.data;
  const running = state?.phase === "running";
  const busy =
    BUSY_PHASES.has(state?.phase ?? "") ||
    startRuntime.isPending ||
    stopRuntime.isPending ||
    installCA.isPending ||
    removeCA.isPending;
  const enabledCount = Object.values(providersQuery.data ?? {}).filter(
    (provider) => provider.settingsConfig.enabled,
  ).length;
  const canStart = enabledCount > 0 && !providersQuery.isLoading;

  const reportError = (title: string, error: unknown) => {
    toast.error(title, {
      description: extractErrorMessage(error) || undefined,
    });
  };

  const start = async () => {
    try {
      await startRuntime.mutateAsync(undefined);
      toast.success(t("cursor.runtime.toast.started"));
    } catch (error) {
      reportError(t("cursor.runtime.error.startFailed"), error);
    }
  };

  const stop = async () => {
    try {
      await stopRuntime.mutateAsync(undefined);
      toast.success(t("cursor.runtime.toast.stopped"));
    } catch (error) {
      reportError(t("cursor.runtime.error.stopFailed"), error);
    }
  };

  const handleToggle = (checked: boolean) => {
    if (!checked) {
      void stop();
      return;
    }
    if (!state?.caInstalled) {
      setTrustDialogOpen(true);
      return;
    }
    void start();
  };

  const handleInstallCAAndStart = async () => {
    setTrustDialogOpen(false);
    try {
      await installCA.mutateAsync(undefined);
      await startRuntime.mutateAsync(undefined);
      toast.success(t("cursor.runtime.toast.trustedAndStarted"));
    } catch (error) {
      reportError(t("cursor.runtime.error.enableFailed"), error);
    }
  };

  const handleInstallCA = async () => {
    try {
      await installCA.mutateAsync(undefined);
      toast.success(t("cursor.runtime.toast.caInstalled"));
    } catch (error) {
      reportError(t("cursor.runtime.error.caInstallFailed"), error);
    }
  };

  const handleRemoveCA = async () => {
    try {
      await removeCA.mutateAsync(undefined);
      setCADialogOpen(false);
      toast.success(t("cursor.runtime.toast.caRemoved"));
    } catch (error) {
      reportError(t("cursor.runtime.error.caRemoveFailed"), error);
    }
  };

  const tooltipText = running
    ? t("cursor.runtime.tooltip.running", { count: enabledCount })
    : canStart
      ? t("cursor.runtime.tooltip.start", { count: enabledCount })
      : t("cursor.runtime.tooltip.noModels");

  return (
    <>
      <div
        className={cn(
          "flex h-8 items-center gap-1 rounded-lg bg-muted/50 px-1.5 transition-all",
          className,
        )}
        title={tooltipText}
      >
        {busy || runtimeQuery.isLoading ? (
          <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
        ) : (
          <Radio
            className={cn(
              "h-4 w-4 transition-colors",
              running
                ? "animate-pulse text-emerald-500"
                : "text-muted-foreground",
            )}
          />
        )}
        <Switch
          checked={running}
          onCheckedChange={handleToggle}
          disabled={busy || (!running && !canStart)}
          aria-label={t("cursor.runtime.toggleAriaLabel")}
        />
      </div>

      <Button
        variant="ghost"
        size="icon"
        className={cn(
          "h-8 w-8 shrink-0",
          state?.caInstalled
            ? "text-emerald-600 dark:text-emerald-400"
            : "text-amber-600 dark:text-amber-400",
        )}
        onClick={() => setCADialogOpen(true)}
        disabled={runtimeQuery.isLoading}
        title={
          state?.caInstalled
            ? t("cursor.runtime.ca.manage")
            : t("cursor.runtime.ca.install")
        }
        aria-label={
          state?.caInstalled
            ? t("cursor.runtime.ca.manage")
            : t("cursor.runtime.ca.install")
        }
      >
        {state?.caInstalled ? (
          <ShieldCheck className="h-4 w-4" />
        ) : (
          <ShieldAlert className="h-4 w-4" />
        )}
      </Button>

      <CursorCAManagementDialog
        open={caDialogOpen}
        state={state}
        busy={busy}
        running={running}
        onOpenChange={setCADialogOpen}
        onInstall={() => void handleInstallCA()}
        onRemove={() => void handleRemoveCA()}
      />

      <CursorTrustDialog
        open={trustDialogOpen}
        platform={state?.platform}
        busy={busy}
        onOpenChange={setTrustDialogOpen}
        onConfirm={() => void handleInstallCAAndStart()}
      />
    </>
  );
}

function CursorCAManagementDialog({
  open,
  state,
  busy,
  running,
  onOpenChange,
  onInstall,
  onRemove,
}: {
  open: boolean;
  state?: CursorRuntimeState;
  busy: boolean;
  running: boolean;
  onOpenChange: (open: boolean) => void;
  onInstall: () => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent zIndex="top">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <KeyRound className="h-5 w-5 text-blue-500" />
            {t("cursor.runtime.ca.dialogTitle")}
          </DialogTitle>
          <DialogDescription>
            {state?.caInstalled
              ? t("cursor.runtime.ca.installedDescription")
              : t("cursor.runtime.ca.requiredDescription")}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3 px-6 py-5 text-sm">
          <div className="flex items-center justify-between gap-3 rounded-lg border border-border-default bg-muted/30 p-3">
            <span className="text-muted-foreground">
              {t("cursor.runtime.ca.status")}
            </span>
            <span className="font-medium">
              {state?.caInstalled
                ? t("cursor.runtime.ca.trusted")
                : t("cursor.runtime.ca.notTrusted")}
            </span>
          </div>
          {state?.caFingerprint && (
            <p
              className="truncate font-mono text-xs text-muted-foreground"
              title={state.caFingerprint}
            >
              {t("cursor.runtime.ca.fingerprint", {
                fingerprint: state.caFingerprint,
              })}
            </p>
          )}
          {running && (
            <p className="text-xs text-muted-foreground">
              {t("cursor.runtime.ca.stopBeforeRemove")}
            </p>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.close")}
          </Button>
          {state?.caInstalled ? (
            <Button
              variant="destructive"
              onClick={onRemove}
              disabled={busy || running}
            >
              {busy ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <ShieldMinus className="mr-2 h-4 w-4" />
              )}
              {t("cursor.runtime.ca.removeAction")}
            </Button>
          ) : (
            <Button onClick={onInstall} disabled={busy}>
              {busy ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <ShieldCheck className="mr-2 h-4 w-4" />
              )}
              {t("cursor.runtime.ca.installAction")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function CursorTrustDialog({
  open,
  platform,
  busy,
  onOpenChange,
  onConfirm,
}: {
  open: boolean;
  platform?: string;
  busy: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const explanation =
    platform === "windows"
      ? t("cursor.runtime.trust.platform.windows")
      : platform === "linux"
        ? t("cursor.runtime.trust.platform.linux")
        : t("cursor.runtime.trust.platform.macos");

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent zIndex="top">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <KeyRound className="h-5 w-5 text-blue-500" />
            {t("cursor.runtime.trust.dialogTitle")}
          </DialogTitle>
          <DialogDescription>
            {t("cursor.runtime.trust.description")}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3 px-6 py-5 text-sm">
          <p>{explanation}</p>
          <div className="rounded-lg border border-border-default bg-muted/30 p-3 text-muted-foreground">
            {t("cursor.runtime.trust.privateKeyNotice")}
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common.cancel")}
          </Button>
          <Button onClick={onConfirm} disabled={busy}>
            {busy ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <BadgeCheck className="mr-2 h-4 w-4" />
            )}
            {t("cursor.runtime.trust.confirmAction")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
