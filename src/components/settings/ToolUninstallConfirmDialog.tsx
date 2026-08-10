import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { AlertTriangle, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ToolInstallationReport } from "@/lib/api/settings";
import { ToolInstallRow } from "./ToolInstallRow";

interface ToolUninstallConfirmDialogProps {
  isOpen: boolean;
  /** The probe report for the tool being uninstalled, or null when probing failed
   * (in which case a generic confirm without a command preview is shown). */
  plan: ToolInstallationReport | null;
  displayName: (tool: string) => string;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * Uninstall confirmation. Uninstall is destructive, so it always prompts (unlike
 * upgrade, which only prompts when ≥2 installs are detected). Three modes:
 * - `plan` null (probe failed): generic "uninstall {tool}?" confirm, no command preview.
 * - `plan.uninstall_supported`: show the anchored uninstall command + a confirm button.
 *   When ≥2 installs exist, warn that only the command-line default install is removed.
 * - `!plan.uninstall_supported` (for example grok native): show manual-removal guidance and
 *   only a Close button — no auto-uninstall is executed.
 */
export function ToolUninstallConfirmDialog({
  isOpen,
  plan,
  displayName,
  onConfirm,
  onCancel,
}: ToolUninstallConfirmDialogProps) {
  const { t } = useTranslation();

  const supported = plan?.uninstall_supported ?? true;
  const multipleInstalls = (plan?.installs.length ?? 0) >= 2;

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open) onCancel();
      }}
    >
      <DialogContent className="max-w-md" zIndex="alert">
        <DialogHeader className="space-y-2 border-b-0 bg-transparent pb-0">
          <DialogTitle className="flex items-center gap-2 text-base font-semibold">
            <AlertTriangle className="h-5 w-5 text-yellow-500" />
            {t("settings.toolUninstallConfirmTitle")}
          </DialogTitle>
          <DialogDescription className="text-sm leading-relaxed">
            {supported
              ? t("settings.toolUninstallConfirmHint")
              : t("settings.toolUninstallNotSupportedHint", {
                  tool: plan ? displayName(plan.tool) : "",
                })}
          </DialogDescription>
        </DialogHeader>

        <div className="max-h-[50vh] space-y-3 overflow-y-auto">
          {plan && (
            <div className="space-y-1.5 rounded-lg border border-border bg-background/40 p-2.5">
              <div className="text-xs font-medium">
                {displayName(plan.tool)}
              </div>
              {multipleInstalls && supported && (
                <div className="text-[10px] leading-snug text-yellow-600 dark:text-yellow-400">
                  {t("settings.toolUninstallConfirmMultipleHint")}
                </div>
              )}
              {supported && (
                <ul className="space-y-1">
                  {plan.installs.map((inst) => (
                    <li key={inst.path}>
                      <ToolInstallRow inst={inst} />
                    </li>
                  ))}
                </ul>
              )}
              {supported && plan.uninstall_command && (
                <div className="space-y-0.5">
                  <div className="text-[10px] text-muted-foreground">
                    {t("settings.toolUninstallWillRun")}
                  </div>
                  <code
                    className="block truncate rounded bg-background/80 px-1.5 py-0.5 font-mono text-[10px] text-foreground"
                    title={plan.uninstall_command}
                  >
                    {plan.uninstall_command}
                  </code>
                </div>
              )}
            </div>
          )}
        </div>

        <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
          <Button variant="outline" onClick={onCancel}>
            {supported ? t("common.cancel") : t("common.close")}
          </Button>
          {supported && (
            <Button variant="destructive" onClick={onConfirm}>
              <Trash2 className="h-3.5 w-3.5" />
              {t("settings.toolUninstallConfirmBtn")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
