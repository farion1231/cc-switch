import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Pencil, RotateCcw, Check, X, Download, Trash2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useBackupManager } from "@/hooks/useBackupManager";
import { extractErrorMessage } from "@/utils/errorUtils";

interface BackupListSectionProps {
  backupIntervalHours?: number;
  backupRetainCount?: number;
  onSettingsChange: (updates: {
    backupIntervalHours?: number;
    backupRetainCount?: number;
  }) => void;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatBackupDate(isoString: string): string {
  try {
    const date = new Date(isoString);
    return date.toLocaleString();
  } catch {
    return isoString;
  }
}

/** Parse display name from backup filename */
function getDisplayName(filename: string): string {
  // Try to parse db_backup_YYYYMMDD_HHMMSS format
  const match = filename.match(
    /^db_backup_(\d{4})(\d{2})(\d{2})_(\d{2})(\d{2})(\d{2})(?:_\d+)?\.db$/,
  );
  if (match) {
    const [, y, m, d, hh, mm, ss] = match;
    return `${y}-${m}-${d} ${hh}:${mm}:${ss}`;
  }
  // Otherwise show filename without .db suffix
  return filename.replace(/\.db$/, "");
}

export function BackupListSection({
  backupIntervalHours,
  backupRetainCount,
  onSettingsChange,
}: BackupListSectionProps) {
  const { t } = useTranslation();
  const {
    backups,
    isLoading,
    create,
    isCreating,
    restore,
    isRestoring,
    rename,
    isRenaming,
    remove,
    isDeleting,
  } = useBackupManager();
  const [confirmFilename, setConfirmFilename] = useState<string | null>(null);
  const [deleteFilename, setDeleteFilename] = useState<string | null>(null);
  const [editingFilename, setEditingFilename] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");

  const handleRestore = async () => {
    if (!confirmFilename) return;
    try {
      const safetyId = await restore(confirmFilename);
      setConfirmFilename(null);
      toast.success(
        t("settings.backupManager.restoreSuccess", {
          defaultValue: "Restore successful! Safety backup created",
        }),
        {
          description: safetyId
            ? `${t("settings.backupManager.safetyBackupId", { defaultValue: "Safety Backup ID" })}: ${safetyId}`
            : undefined,
          duration: 6000,
          closeButton: true,
        },
      );
    } catch (error) {
      const detail =
        extractErrorMessage(error) ||
        t("settings.backupManager.restoreFailed", {
          defaultValue: "Restore failed",
        });
      toast.error(detail);
    }
  };

  const handleStartRename = (filename: string) => {
    setEditingFilename(filename);
    setEditValue(getDisplayName(filename));
  };

  const handleCancelRename = () => {
    setEditingFilename(null);
    setEditValue("");
  };

  const handleDelete = async () => {
    if (!deleteFilename) return;
    try {
      await remove(deleteFilename);
      setDeleteFilename(null);
      toast.success(
        t("settings.backupManager.deleteSuccess", {
          defaultValue: "Backup deleted",
        }),
      );
    } catch (error) {
      const detail =
        extractErrorMessage(error) ||
        t("settings.backupManager.deleteFailed", {
          defaultValue: "Delete failed",
        });
      toast.error(detail);
    }
  };

  const handleConfirmRename = async () => {
    if (!editingFilename || !editValue.trim()) return;
    try {
      await rename({ oldFilename: editingFilename, newName: editValue.trim() });
      setEditingFilename(null);
      setEditValue("");
      toast.success(
        t("settings.backupManager.renameSuccess", {
          defaultValue: "Backup renamed",
        }),
      );
    } catch (error) {
      const detail =
        extractErrorMessage(error) ||
        t("settings.backupManager.renameFailed", {
          defaultValue: "Rename failed",
        });
      toast.error(detail);
    }
  };

  const intervalValue = String(backupIntervalHours ?? 24);
  const retainValue = String(backupRetainCount ?? 10);

  return (
    <div className="space-y-4">
      {/* Backup policy settings */}
      <div className="grid grid-cols-2 gap-4">
        <div className="space-y-2">
          <Label className="text-sm">
            {t("settings.backupManager.intervalLabel", {
              defaultValue: "Auto-backup Interval",
            })}
          </Label>
          <Select
            value={intervalValue}
            onValueChange={(v) =>
              onSettingsChange({ backupIntervalHours: Number(v) })
            }
          >
            <SelectTrigger className="h-9">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="0">
                {t("settings.backupManager.intervalDisabled", {
                  defaultValue: "Disabled",
                })}
              </SelectItem>
              <SelectItem value="6">
                {t("settings.backupManager.intervalHours", {
                  hours: 6,
                  defaultValue: "6 hours",
                })}
              </SelectItem>
              <SelectItem value="12">
                {t("settings.backupManager.intervalHours", {
                  hours: 12,
                  defaultValue: "12 hours",
                })}
              </SelectItem>
              <SelectItem value="24">
                {t("settings.backupManager.intervalHours", {
                  hours: 24,
                  defaultValue: "24 hours",
                })}
              </SelectItem>
              <SelectItem value="48">
                {t("settings.backupManager.intervalHours", {
                  hours: 48,
                  defaultValue: "48 hours",
                })}
              </SelectItem>
              <SelectItem value="168">
                {t("settings.backupManager.intervalDays", {
                  days: 7,
                  defaultValue: "7 days",
                })}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div className="space-y-2">
          <Label className="text-sm">
            {t("settings.backupManager.retainLabel", {
              defaultValue: "Backup Retention",
            })}
          </Label>
          <Select
            value={retainValue}
            onValueChange={(v) =>
              onSettingsChange({ backupRetainCount: Number(v) })
            }
          >
            <SelectTrigger className="h-9">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {[3, 5, 10, 15, 20, 30, 50].map((n) => (
                <SelectItem key={n} value={String(n)}>
                  {n}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {/* Backup list */}
      <div>
        <div className="flex items-center justify-between mb-2">
          <h4 className="text-sm font-medium">
            {t("settings.backupManager.title", {
              defaultValue: "Database Backups",
            })}
          </h4>
          <Button
            variant="outline"
            size="sm"
            className="h-7 px-2 text-xs"
            disabled={isCreating || isRestoring}
            onClick={async () => {
              try {
                await create();
                toast.success(
                  t("settings.backupManager.createSuccess", {
                    defaultValue: "Backup created successfully",
                  }),
                );
              } catch (error) {
                const detail =
                  extractErrorMessage(error) ||
                  t("settings.backupManager.createFailed", {
                    defaultValue: "Backup failed",
                  });
                toast.error(detail);
              }
            }}
          >
            <Download className="h-3 w-3 mr-1" />
            {isCreating
              ? t("settings.backupManager.creating", {
                  defaultValue: "Backing up...",
                })
              : t("settings.backupManager.createBackup", {
                  defaultValue: "Backup Now",
                })}
          </Button>
        </div>

        {isLoading ? (
          <div className="text-sm text-muted-foreground py-2">Loading...</div>
        ) : backups.length === 0 ? (
          <div className="text-sm text-muted-foreground py-2">
            {t("settings.backupManager.empty", {
              defaultValue: "No backups yet",
            })}
          </div>
        ) : (
          <div className="space-y-1.5 max-h-48 overflow-y-auto">
            {backups.map((backup) => (
              <div
                key={backup.filename}
                className="flex items-center justify-between gap-2 px-3 py-2 rounded-lg bg-muted/30 hover:bg-muted/50 transition-colors text-sm"
              >
                <div className="flex-1 min-w-0">
                  {editingFilename === backup.filename ? (
                    <div className="flex items-center gap-1.5">
                      <Input
                        value={editValue}
                        onChange={(e) => setEditValue(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") handleConfirmRename();
                          if (e.key === "Escape") handleCancelRename();
                        }}
                        className="h-7 text-xs"
                        placeholder={t(
                          "settings.backupManager.namePlaceholder",
                          { defaultValue: "Enter new name" },
                        )}
                        autoFocus
                        disabled={isRenaming}
                      />
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-6 w-6 shrink-0"
                        onClick={handleConfirmRename}
                        disabled={isRenaming || !editValue.trim()}
                      >
                        <Check className="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-6 w-6 shrink-0"
                        onClick={handleCancelRename}
                        disabled={isRenaming}
                      >
                        <X className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  ) : (
                    <>
                      <div className="font-mono text-xs truncate">
                        {getDisplayName(backup.filename)}
                      </div>
                      <div className="text-xs text-muted-foreground">
                        {formatBackupDate(backup.createdAt)} &middot;{" "}
                        {formatBytes(backup.sizeBytes)}
                      </div>
                    </>
                  )}
                </div>
                {editingFilename !== backup.filename && (
                  <div className="flex items-center gap-1 shrink-0">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      onClick={() => handleStartRename(backup.filename)}
                      disabled={isRestoring || isRenaming || isDeleting}
                      title={t("settings.backupManager.rename", {
                        defaultValue: "Rename",
                      })}
                    >
                      <Pencil className="h-3 w-3" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 text-destructive hover:text-destructive"
                      onClick={() => setDeleteFilename(backup.filename)}
                      disabled={isRestoring || isDeleting}
                      title={t("settings.backupManager.delete", {
                        defaultValue: "Delete",
                      })}
                    >
                      <Trash2 className="h-3 w-3" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-7 px-2 text-xs"
                      disabled={isRestoring || isDeleting}
                      onClick={() => setConfirmFilename(backup.filename)}
                    >
                      <RotateCcw className="h-3 w-3 mr-1" />
                      {isRestoring
                        ? t("settings.backupManager.restoring", {
                            defaultValue: "Restoring...",
                          })
                        : t("settings.backupManager.restore", {
                            defaultValue: "Restore",
                          })}
                    </Button>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Restore Confirmation Dialog */}
      <Dialog
        open={!!confirmFilename}
        onOpenChange={(open) => !open && setConfirmFilename(null)}
      >
        <DialogContent className="max-w-md" zIndex="alert">
          <DialogHeader>
            <DialogTitle>
              {t("settings.backupManager.confirmTitle", {
                defaultValue: "Confirm Restore",
              })}
            </DialogTitle>
            <DialogDescription>
              {t("settings.backupManager.confirmMessage", {
                defaultValue:
                  "Restoring this backup will overwrite the current database. A safety backup will be created first.",
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setConfirmFilename(null)}
              disabled={isRestoring}
            >
              {t("common.cancel", { defaultValue: "Cancel" })}
            </Button>
            <Button onClick={handleRestore} disabled={isRestoring}>
              {isRestoring
                ? t("settings.backupManager.restoring", {
                    defaultValue: "Restoring...",
                  })
                : t("settings.backupManager.restore", {
                    defaultValue: "Restore",
                  })}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Confirmation Dialog */}
      <Dialog
        open={!!deleteFilename}
        onOpenChange={(open) => !open && setDeleteFilename(null)}
      >
        <DialogContent className="max-w-md" zIndex="alert">
          <DialogHeader>
            <DialogTitle>
              {t("settings.backupManager.deleteConfirmTitle", {
                defaultValue: "Confirm Delete",
              })}
            </DialogTitle>
            <DialogDescription>
              {t("settings.backupManager.deleteConfirmMessage", {
                defaultValue:
                  "This backup will be permanently deleted. This action cannot be undone.",
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setDeleteFilename(null)}
              disabled={isDeleting}
            >
              {t("common.cancel", { defaultValue: "Cancel" })}
            </Button>
            <Button
              variant="destructive"
              onClick={handleDelete}
              disabled={isDeleting}
            >
              {isDeleting
                ? t("settings.backupManager.deleting", {
                    defaultValue: "Deleting...",
                  })
                : t("settings.backupManager.delete", {
                    defaultValue: "Delete",
                  })}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
