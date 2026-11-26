import { useEffect, useRef } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ToolInstallationReport } from "@/lib/api/settings";
import { ToolInstallRow } from "./ToolInstallRow";

export type ToolUpgradePreview = Pick<
  ToolInstallationReport,
  | "tool"
  | "installs"
  | "command"
  | "anchored"
  | "update_plan"
  | "confirmation_token"
> & { target_version?: string | null };

interface ToolUpgradeConfirmDialogProps {
  isOpen: boolean;
  plans: ToolUpgradePreview[];
  displayName: (tool: string) => string;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ToolUpgradeConfirmDialog({
  isOpen,
  plans,
  displayName,
  onConfirm,
  onCancel,
}: ToolUpgradeConfirmDialogProps) {
  const { t } = useTranslation();
  const titleRef = useRef<HTMLHeadingElement>(null);

  // 队首切换后移开操作按钮的焦点，按住 Enter/Space 不能继续处理下一项。
  useEffect(() => {
    if (isOpen) titleRef.current?.focus();
  }, [isOpen, plans]);

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open) onCancel();
      }}
    >
      <DialogContent
        className="max-w-md"
        zIndex="alert"
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          titleRef.current?.focus();
        }}
      >
        <DialogHeader className="space-y-2 border-b-0 bg-transparent pb-0">
          <DialogTitle
            ref={titleRef}
            tabIndex={-1}
            className="flex items-center gap-2 text-base font-semibold"
          >
            <AlertTriangle className="h-5 w-5 text-warning-text" />
            {t("settings.toolUpgradeConfirmTitle")}
          </DialogTitle>
          <DialogDescription className="text-sm leading-relaxed">
            {t("settings.toolUpgradeConfirmHint")}
          </DialogDescription>
        </DialogHeader>

        <div className="max-h-[50vh] space-y-3 overflow-y-auto">
          {plans.map((plan) => (
            <div
              key={`${plan.tool}:${plan.update_plan?.path ?? ""}`}
              className="space-y-1.5 rounded-lg border border-transparent bg-warning-soft p-2.5"
            >
              <div className="text-xs font-medium">
                {displayName(plan.tool)}
              </div>
              {!plan.anchored && (
                <div className="text-badge leading-snug text-warning-text">
                  {t("settings.toolUpgradeUnanchoredHint")}
                </div>
              )}
              <ul className="space-y-1">
                {plan.installs.map((inst) => (
                  <li key={inst.path}>
                    <ToolInstallRow
                      inst={inst}
                      updateTarget={
                        plan.update_plan
                          ? plan.update_plan.path === inst.path
                          : plan.anchored && inst.is_path_default
                      }
                    />
                  </li>
                ))}
              </ul>
              {plan.command && (
                <div className="space-y-0.5">
                  <div className="text-badge text-fg-2">
                    {t("settings.toolUpgradeWillRun")}
                  </div>
                  <code
                    className="block truncate rounded bg-surface px-1.5 py-0.5 font-mono text-badge text-fg-1"
                    title={plan.command}
                  >
                    {plan.command}
                  </code>
                </div>
              )}
            </div>
          ))}
        </div>

        <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
          <Button
            variant="outline"
            onClick={(event) => {
              if (event.detail <= 1) onCancel();
            }}
          >
            {t("common.cancel")}
          </Button>
          <Button
            onClick={(event) => {
              // 第二次 click 可能已经面对下一项计划，不能把双击视作两次授权。
              if (event.detail <= 1) onConfirm();
            }}
          >
            {t("settings.toolUpgradeConfirmBtn")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
