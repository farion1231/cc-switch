import { useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, ChevronDown, ChevronUp, X, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import type { EnvConflict } from "@/types/env";
import { deleteEnvVars } from "@/lib/api/env";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface EnvWarningBannerProps {
  conflicts: EnvConflict[];
  onDismiss: () => void;
  onDeleted: () => void;
}

export function EnvWarningBanner({
  conflicts,
  onDismiss,
  onDeleted,
}: EnvWarningBannerProps) {
  const { t } = useTranslation();
  const [isExpanded, setIsExpanded] = useState(false);
  const [selectedConflicts, setSelectedConflicts] = useState<Set<string>>(
    new Set(),
  );
  const [isDeleting, setIsDeleting] = useState(false);
  const [showConfirmDialog, setShowConfirmDialog] = useState(false);

  if (conflicts.length === 0) {
    return null;
  }

  const toggleSelection = (key: string) => {
    const newSelection = new Set(selectedConflicts);
    if (newSelection.has(key)) {
      newSelection.delete(key);
    } else {
      newSelection.add(key);
    }
    setSelectedConflicts(newSelection);
  };

  const toggleSelectAll = () => {
    if (selectedConflicts.size === conflicts.length) {
      setSelectedConflicts(new Set());
    } else {
      setSelectedConflicts(
        new Set(conflicts.map((c) => `${c.varName}:${c.sourcePath}`)),
      );
    }
  };

  const handleDelete = async () => {
    setShowConfirmDialog(false);
    setIsDeleting(true);

    try {
      const conflictsToDelete = conflicts.filter((c) =>
        selectedConflicts.has(`${c.varName}:${c.sourcePath}`),
      );

      if (conflictsToDelete.length === 0) {
        toast.warning(t("env.error.noSelection"));
        return;
      }

      const backupInfo = await deleteEnvVars(conflictsToDelete);

      toast.success(t("env.delete.success"), {
        description: t("env.backup.location", {
          path: backupInfo.backupPath,
        }),
        duration: 5000,
        closeButton: true,
      });

      // 清空选择并通知父组件
      setSelectedConflicts(new Set());
      onDeleted();
    } catch (error) {
      console.error("删除环境变量失败:", error);
      toast.error(t("env.delete.error"), {
        description: String(error),
      });
    } finally {
      setIsDeleting(false);
    }
  };

  const getSourceDescription = (conflict: EnvConflict): string => {
    if (conflict.sourceType === "system") {
      if (conflict.sourcePath.includes("HKEY_CURRENT_USER")) {
        return t("env.source.userRegistry");
      } else if (conflict.sourcePath.includes("HKEY_LOCAL_MACHINE")) {
        return t("env.source.systemRegistry");
      } else {
        return t("env.source.systemEnv");
      }
    } else {
      return conflict.sourcePath;
    }
  };

  return (
    <>
      <div className="fixed top-0 left-0 right-0 z-[100] bg-yellow-50 dark:bg-yellow-950 border-b border-yellow-200 dark:border-yellow-900 shadow-lg animate-slide-down">
        <div className="container mx-auto px-4 py-3">
          <div className="flex items-start gap-3">
            <AlertTriangle className="h-5 w-5 text-yellow-600 dark:text-yellow-500 flex-shrink-0 mt-0.5" />

            <div className="flex-1 min-w-0">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <h3 className="text-sm font-semibold text-yellow-900 dark:text-yellow-100">
                    {t("env.warning.title")}
                  </h3>
                  <p className="text-sm text-yellow-800 dark:text-yellow-200 mt-0.5">
                    {t("env.warning.description", { count: conflicts.length })}
                  </p>
                </div>

                <div className="flex items-center gap-2 flex-shrink-0">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setIsExpanded(!isExpanded)}
                    className="text-yellow-900 dark:text-yellow-100 hover:bg-yellow-100 dark:hover:bg-yellow-900/50"
                  >
                    {isExpanded ? (
                      <>
                        {t("env.actions.collapse")}
                        <ChevronUp className="h-4 w-4 ml-1" />
                      </>
                    ) : (
                      <>
                        {t("env.actions.expand")}
                        <ChevronDown className="h-4 w-4 ml-1" />
                      </>
                    )}
                  </Button>

                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={onDismiss}
                    className="text-yellow-900 dark:text-yellow-100 hover:bg-yellow-100 dark:hover:bg-yellow-900/50"
                  >
                    <X className="h-4 w-4" />
                  </Button>
                </div>
              </div>

              {isExpanded && (
                <div className="mt-4 space-y-3">
                  <div className="flex items-center gap-2 pb-2 border-b border-yellow-200 dark:border-yellow-900/50">
                    <Checkbox
                      id="select-all"
                      checked={selectedConflicts.size === conflicts.length}
                      onCheckedChange={toggleSelectAll}
                    />
                    <label
                      htmlFor="select-all"
                      className="text-sm font-medium text-yellow-900 dark:text-yellow-100 cursor-pointer"
                    >
                      {t("env.actions.selectAll")}
                    </label>
                  </div>

                  <div className="max-h-96 overflow-y-auto space-y-2">
                    {conflicts.map((conflict) => {
                      const key = `${conflict.varName}:${conflict.sourcePath}`;
                      return (
                        <div
                          key={key}
                          className="flex items-start gap-3 p-3 bg-white dark:bg-gray-900 rounded-md border border-yellow-200 dark:border-yellow-900/50"
                        >
                          <Checkbox
                            id={key}
                            checked={selectedConflicts.has(key)}
                            onCheckedChange={() => toggleSelection(key)}
                          />

                          <div className="flex-1 min-w-0">
                            <label
                              htmlFor={key}
                              className="block text-sm font-medium text-foreground cursor-pointer"
                            >
                              {conflict.varName}
                            </label>
                            <p className="text-xs text-muted-foreground mt-1 break-all">
                              {t("env.field.value")}: {conflict.varValue}
                            </p>
                            <p className="text-xs text-muted-foreground mt-1">
                              {t("env.field.source")}:{" "}
                              {getSourceDescription(conflict)}
                            </p>
                          </div>
                        </div>
                      );
                    })}
                  </div>

                  <div className="flex items-center justify-end gap-2 pt-2 border-t border-yellow-200 dark:border-yellow-900/50">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => setSelectedConflicts(new Set())}
                      disabled={selectedConflicts.size === 0}
                      className="text-yellow-900 dark:text-yellow-100 border-yellow-300 dark:border-yellow-800"
                    >
                      {t("env.actions.clearSelection")}
                    </Button>

                    <Button
                      variant="destructive"
                      size="sm"
                      onClick={() => setShowConfirmDialog(true)}
                      disabled={selectedConflicts.size === 0 || isDeleting}
                      className="gap-1"
                    >
                      <Trash2 className="h-4 w-4" />
                      {isDeleting
                        ? t("env.actions.deleting")
                        : t("env.actions.deleteSelected", {
                            count: selectedConflicts.size,
                          })}
                    </Button>
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>
      </div>

      <Dialog open={showConfirmDialog} onOpenChange={setShowConfirmDialog}>
        <DialogContent className="max-w-md" zIndex="top">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              {t("env.confirm.title")}
            </DialogTitle>
            <DialogDescription className="space-y-2">
              <p>
                {t("env.confirm.message", { count: selectedConflicts.size })}
              </p>
              <p className="text-sm text-muted-foreground">
                {t("env.confirm.backupNotice")}
              </p>
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setShowConfirmDialog(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button variant="destructive" onClick={handleDelete}>
              {t("env.confirm.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
