import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { ChevronRight, FolderOpen, Trash2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { HoverTip } from "@/components/ui/hover-tip";
import { backupsApi, type BackupLocation } from "@/lib/api/settings";
import { useBackupLocations } from "@/hooks/useBackupManager";
import { extractErrorMessage } from "@/utils/errorUtils";

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

/** 各类备份的位置与大小，删不删由用户决定（类别与路径由后端给出） */
export function BackupStorageSection() {
  const { t } = useTranslation();
  const { locations, isLoading, isError, remove, isDeleting } =
    useBackupLocations();
  const [pending, setPending] = useState<BackupLocation | null>(null);
  // 默认折叠：备份类别多时列表较长，标题行仍显示合计大小
  const [open, setOpen] = useState(false);

  const itemText = (id: string, field: "name" | "hint") =>
    t(`settings.backupStorage.items.${id}.${field}`, { defaultValue: id });
  const revealLabel = t("settings.backupStorage.reveal");
  const deleteLabel = t("settings.backupStorage.delete");
  const totalBytes = locations.reduce((sum, l) => sum + l.sizeBytes, 0);

  const handleReveal = async (id: string) => {
    try {
      await backupsApi.revealBackupLocation(id);
    } catch (error) {
      toast.error(
        extractErrorMessage(error) || t("settings.backupStorage.revealFailed"),
      );
    }
  };

  const handleDelete = async () => {
    if (!pending) return;
    try {
      const freed = await remove(pending.id);
      setPending(null);
      toast.success(
        t("settings.backupStorage.deleteSuccess", {
          size: formatBytes(freed),
        }),
      );
    } catch (error) {
      toast.error(
        extractErrorMessage(error) || t("settings.backupStorage.deleteFailed"),
      );
    }
  };

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <CollapsibleTrigger asChild>
        <button
          type="button"
          className="flex w-full items-center justify-between gap-2 rounded-md py-1 text-left"
        >
          <span className="flex items-center gap-1 text-sm font-medium">
            <ChevronRight
              className={`h-3.5 w-3.5 text-fg-2 transition-transform ${
                open ? "rotate-90" : ""
              }`}
            />
            {t("settings.backupStorage.title")}
          </span>
          {locations.length > 0 && (
            <span className="text-xs text-fg-2">
              {t("settings.backupStorage.total", {
                size: formatBytes(totalBytes),
              })}
            </span>
          )}
        </button>
      </CollapsibleTrigger>

      <CollapsibleContent className="pt-2">
        {isLoading ? (
          <div className="text-sm text-fg-2 py-2">Loading...</div>
        ) : isError ? (
          <div className="text-sm text-fg-2 py-2">
            {t("settings.backupStorage.loadFailed")}
          </div>
        ) : (
          <div className="space-y-1.5">
            {locations.map((location) => (
              <div
                key={location.id}
                className="flex items-center justify-between gap-2 px-3 py-2 rounded-lg bg-subtle text-sm"
              >
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-1.5 text-xs font-medium">
                    <span className="truncate">
                      {itemText(location.id, "name")}
                    </span>
                    {location.id.startsWith("legacy") && (
                      <span className="shrink-0 rounded px-1 py-px text-[10px] font-normal text-fg-2 border border-border">
                        {t("settings.backupStorage.legacy")}
                      </span>
                    )}
                  </div>
                  <div
                    className="font-mono text-xs text-fg-2 truncate"
                    title={location.path}
                  >
                    {location.path}
                  </div>
                  <div className="text-xs text-fg-2">
                    {formatBytes(location.sizeBytes)} &middot;{" "}
                    {t("settings.backupStorage.itemCount", {
                      count: location.itemCount,
                    })}
                    {!location.deletable && (
                      <> &middot; {t("settings.backupStorage.managedAbove")}</>
                    )}
                  </div>
                </div>
                <div className="flex items-center gap-1 shrink-0">
                  <HoverTip content={revealLabel}>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      onClick={() => void handleReveal(location.id)}
                      aria-label={revealLabel}
                    >
                      <FolderOpen className="h-3 w-3" />
                    </Button>
                  </HoverTip>
                  {location.deletable && (
                    <HoverTip content={deleteLabel}>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7 text-destructive hover:text-destructive"
                        onClick={() => setPending(location)}
                        disabled={isDeleting}
                        aria-label={deleteLabel}
                      >
                        <Trash2 className="h-3 w-3" />
                      </Button>
                    </HoverTip>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </CollapsibleContent>

      <Dialog
        open={!!pending}
        onOpenChange={(open) => !open && !isDeleting && setPending(null)}
      >
        <DialogContent className="max-w-md" zIndex="alert">
          <DialogHeader>
            <DialogTitle>
              {pending &&
                t("settings.backupStorage.deleteConfirmTitle", {
                  name: itemText(pending.id, "name"),
                })}
            </DialogTitle>
            <DialogDescription asChild>
              <div className="space-y-2">
                {pending && <p>{itemText(pending.id, "hint")}</p>}
                {pending && (
                  <p>
                    {t("settings.backupStorage.deleteConfirmMessage", {
                      path: pending.path,
                      size: formatBytes(pending.sizeBytes),
                    })}
                  </p>
                )}
              </div>
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setPending(null)}
              disabled={isDeleting}
            >
              {t("common.cancel", { defaultValue: "Cancel" })}
            </Button>
            <Button
              variant="destructive"
              onClick={() => void handleDelete()}
              disabled={isDeleting}
            >
              {isDeleting
                ? t("settings.backupStorage.deleting")
                : t("settings.backupStorage.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Collapsible>
  );
}
