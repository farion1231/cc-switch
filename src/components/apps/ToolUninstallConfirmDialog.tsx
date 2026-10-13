import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { settingsApi } from "@/lib/api";
import type { ToolUninstallPlan } from "@/lib/api/settings";
import { extractErrorMessage } from "@/utils/errorUtils";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  toolDisplayName,
  type ToolName,
  type WslShellPreference,
} from "./useToolManagement";

export function ToolUninstallConfirmDialog({
  tool,
  wslShellByTool,
  onConfirm,
  onCancel,
  onManualUninstall,
}: {
  tool: ToolName;
  wslShellByTool: Record<string, WslShellPreference>;
  onConfirm: (plan: ToolUninstallPlan) => void;
  onCancel: () => void;
  onManualUninstall: () => void;
}) {
  const { t } = useTranslation();
  const [plan, setPlan] = useState<ToolUninstallPlan | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setPlan(null);
    setError(null);
    void settingsApi.prepareToolUninstall(tool, wslShellByTool).then(
      (result) => {
        if (!cancelled) setPlan(result);
      },
      (reason) => {
        if (!cancelled) setError(extractErrorMessage(reason));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [tool, wslShellByTool]);

  return (
    <Dialog open onOpenChange={(open) => !open && onCancel()}>
      <DialogContent className="max-w-2xl" zIndex="alert">
        <DialogHeader>
          <DialogTitle>
            {t("settings.toolUninstallConfirmTitle", {
              tool: toolDisplayName(tool),
            })}
          </DialogTitle>
          <DialogDescription>
            {t("settings.toolUninstallConfirmHint")}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 space-y-3 overflow-y-auto px-6">
          {error !== null ? (
            <div role="alert" className="space-y-2 text-body">
              <p>{t("settings.toolUninstallManualRequired")}</p>
              {error && <p className="break-words text-fg-2">{error}</p>}
            </div>
          ) : plan ? (
            <>
              {plan.wsl_distro && (
                <p className="text-body">WSL: {plan.wsl_distro}</p>
              )}
              <dl className="space-y-2 text-body">
                <dt className="text-fg-2">{t("settings.toolUninstallPath")}</dt>
                <dd className="break-all font-mono">{plan.path}</dd>
                <dt className="text-fg-2">
                  {t("settings.toolUpgradeWillRun")}
                </dt>
                <dd>
                  <pre className="overflow-x-auto rounded-control border border-border bg-subtle px-3 py-2.5 font-mono text-caption">
                    <code>{plan.command}</code>
                  </pre>
                </dd>
              </dl>
              {plan.native && (
                <p className="text-caption text-fg-2">
                  {t("settings.toolUninstallNativeHint")}
                </p>
              )}
            </>
          ) : (
            <p
              role="status"
              className="flex items-center gap-2 text-body text-fg-2"
            >
              <Loader2 className="h-4 w-4 animate-spin" />
              {t("settings.toolUninstallPreparing")}
            </p>
          )}
        </div>
        <DialogFooter>
          <Button variant="neutral" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          {error !== null && (
            <Button variant="neutral" onClick={onManualUninstall}>
              {t("settings.manualUninstallCommands")}
            </Button>
          )}
          {plan && error === null && (
            <Button variant="destructive" onClick={() => onConfirm(plan)}>
              {t("settings.toolUninstall")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
