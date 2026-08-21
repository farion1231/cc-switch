import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type {
  CopilotCliEnvironmentPreview,
  CopilotCliEnvironmentValue,
} from "@/lib/api";
import type { CopilotUiError } from "@/lib/copilotByokMessages";
import { CopilotErrorMessage } from "./CopilotErrorMessage";

interface CopilotCliEnvironmentDialogProps {
  preview: CopilotCliEnvironmentPreview | null;
  providerName: string;
  pending: boolean;
  error: CopilotUiError | null;
  onConfirm: () => void;
  onCancel: () => void;
}

export function CopilotCliEnvironmentDialog({
  preview,
  providerName,
  pending,
  error,
  onConfirm,
  onCancel,
}: CopilotCliEnvironmentDialogProps) {
  const { t } = useTranslation();
  const displayValue = (
    value: CopilotCliEnvironmentValue,
    sensitive: boolean,
  ) => {
    if (!value.isSet) return t("copilotByok.cli.valueUnset");
    if (sensitive) return t("copilotByok.cli.valueHidden");
    return value.value === "" ? t("copilotByok.cli.valueEmpty") : value.value;
  };

  return (
    <Dialog
      open={preview !== null}
      onOpenChange={(open) => {
        if (!open && !pending) onCancel();
      }}
    >
      <DialogContent className="max-w-2xl" zIndex="alert">
        <DialogHeader>
          <DialogTitle>{t("copilotByok.cli.previewTitle")}</DialogTitle>
          <DialogDescription>
            {t("copilotByok.cli.previewDescription", {
              provider: providerName,
            })}
          </DialogDescription>
        </DialogHeader>
        <div className="max-h-[55vh] space-y-3 overflow-y-auto px-6 py-4">
          {error ? <CopilotErrorMessage error={error} /> : null}
          {preview?.changes.map((change) => (
            <div key={change.name} className="rounded-lg border p-3">
              <p className="break-all font-mono text-xs font-semibold">
                {change.name}
              </p>
              <p className="mt-1 break-all text-xs text-muted-foreground">
                {t("copilotByok.cli.environmentSource")}: {change.source}
              </p>
              <dl className="mt-3 grid grid-cols-3 gap-3 text-xs">
                {(["previous", "current", "desired"] as const).map((key) => (
                  <div key={key}>
                    <dt className="text-muted-foreground">
                      {t(`copilotByok.cli.${key}Value`)}
                    </dt>
                    <dd className="mt-1 whitespace-pre-wrap break-all font-mono">
                      {displayValue(change[key], change.sensitive)}
                    </dd>
                  </div>
                ))}
              </dl>
            </div>
          ))}
          {preview?.integrationConflicts.length ? (
            <div className="rounded-lg border p-3 text-xs">
              <p>{t("copilotByok.cli.integrationChanges")}</p>
              <ul className="mt-2 space-y-1 break-all font-mono">
                {preview.integrationConflicts.map((path) => (
                  <li key={path}>{path}</li>
                ))}
              </ul>
            </div>
          ) : null}
          <p className="text-xs text-muted-foreground">
            {t("copilotByok.cli.previewSafety")}
          </p>
        </div>
        <DialogFooter className="sm:flex-wrap">
          <Button
            className="h-auto min-h-9 whitespace-normal [overflow-wrap:anywhere]"
            variant="outline"
            disabled={pending}
            onClick={onCancel}
          >
            {t("copilotByok.cli.keepEnvironment")}
          </Button>
          <Button
            className="min-w-0 h-auto min-h-9 whitespace-normal [overflow-wrap:anywhere]"
            disabled={pending || !preview}
            onClick={onConfirm}
          >
            {pending ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
            {t("copilotByok.cli.confirmReapply", { provider: providerName })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
