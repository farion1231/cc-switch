import {
  AlertTriangle,
  DownloadCloud,
  Loader2,
  UploadCloud,
} from "lucide-react";
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

export type WebdavSyncResolutionAction = "upload" | "download";

interface WebdavSyncConflictDialogProps {
  open: boolean;
  resolvingAction: WebdavSyncResolutionAction | null;
  onUseLocal: () => void;
  onUseRemote: () => void;
  onCancel: () => void;
}

export function WebdavSyncConflictDialog({
  open,
  resolvingAction,
  onUseLocal,
  onUseRemote,
  onCancel,
}: WebdavSyncConflictDialogProps) {
  const { t } = useTranslation();
  const isResolving = resolvingAction !== null;

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && !isResolving) {
          onCancel();
        }
      }}
    >
      <DialogContent className="max-w-md" zIndex="alert">
        <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
          <DialogTitle className="flex items-center gap-2 text-lg font-semibold">
            <AlertTriangle className="h-5 w-5 text-destructive" />
            {t("settings.webdavSync.conflictDialog.title")}
          </DialogTitle>
          <DialogDescription asChild>
            <div className="space-y-3 text-sm leading-relaxed">
              <p>{t("settings.webdavSync.conflictDialog.message")}</p>
              <div className="rounded-lg border border-border bg-muted/50 p-3 text-xs text-muted-foreground">
                {t("settings.webdavSync.conflictDialog.warning")}
              </div>
            </div>
          </DialogDescription>
        </DialogHeader>
        <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
          <Button variant="outline" onClick={onCancel} disabled={isResolving}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="secondary"
            onClick={onUseRemote}
            disabled={isResolving}
          >
            {resolvingAction === "download" ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <DownloadCloud className="h-3.5 w-3.5" />
            )}
            {t("settings.webdavSync.conflictDialog.useRemote")}
          </Button>
          <Button
            variant="destructive"
            onClick={onUseLocal}
            disabled={isResolving}
          >
            {resolvingAction === "upload" ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <UploadCloud className="h-3.5 w-3.5" />
            )}
            {t("settings.webdavSync.conflictDialog.useLocal")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
