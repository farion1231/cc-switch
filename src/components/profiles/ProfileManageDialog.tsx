import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Pencil, Trash2, X } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  useDeleteProfileMutation,
  useProfilesQuery,
  useUpdateProfileMutation,
} from "@/lib/query/profiles";

interface ProfileManageDialogProps {
  isOpen: boolean;
  onClose: () => void;
}

type PendingConfirm = {
  id: string;
  name: string;
};

/**
 * 项目管理对话框（纯快照式）
 *
 * 项目列表全应用共享，重命名/删除作用于共享实体。
 * 快照内容由切换时的自动保存维护，不提供手动重拍入口。
 */
export function ProfileManageDialog({
  isOpen,
  onClose,
}: ProfileManageDialogProps) {
  const { t } = useTranslation();
  const { data } = useProfilesQuery();
  const updateMutation = useUpdateProfileMutation();
  const deleteMutation = useDeleteProfileMutation();

  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [confirm, setConfirm] = useState<PendingConfirm | null>(null);

  const profiles = data?.profiles ?? [];

  const startRename = (id: string, name: string) => {
    setEditingId(id);
    setEditingName(name);
  };

  const cancelRename = () => {
    setEditingId(null);
    setEditingName("");
  };

  const saveRename = () => {
    const name = editingName.trim();
    if (!name || !editingId) return;
    updateMutation.mutate({ id: editingId, name }, { onSuccess: cancelRename });
  };

  const handleConfirm = () => {
    if (!confirm) return;
    deleteMutation.mutate(confirm.id);
    setConfirm(null);
  };

  return (
    <>
      <Dialog
        open={isOpen}
        onOpenChange={(open) => {
          if (!open) {
            cancelRename();
            onClose();
          }
        }}
      >
        <DialogContent className="max-w-md">
          <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
            <DialogTitle>{t("profiles.manageTitle")}</DialogTitle>
            <DialogDescription>
              {t("profiles.manageDescription")}
            </DialogDescription>
          </DialogHeader>
          <div className="max-h-[50vh] space-y-1 overflow-y-auto px-6 pb-4 pt-3">
            {profiles.length === 0 && (
              <div className="py-4 text-center text-sm text-muted-foreground">
                {t("profiles.empty")}
              </div>
            )}
            {profiles.map((profile) => (
              <div
                key={profile.id}
                className="flex items-center gap-1.5 rounded-md px-2 py-1.5 hover:bg-muted/50"
              >
                {editingId === profile.id ? (
                  <>
                    <Input
                      value={editingName}
                      onChange={(e) => setEditingName(e.target.value)}
                      className="h-7 flex-1"
                      autoFocus
                      onKeyDown={(e) => {
                        if (e.key === "Enter") saveRename();
                        if (e.key === "Escape") cancelRename();
                      }}
                    />
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      title={t("common.confirm")}
                      onClick={saveRename}
                      disabled={!editingName.trim() || updateMutation.isPending}
                    >
                      <Check className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      title={t("common.cancel")}
                      onClick={cancelRename}
                    >
                      <X className="h-3.5 w-3.5" />
                    </Button>
                  </>
                ) : (
                  <>
                    <span className="flex-1 truncate text-sm">
                      {profile.name}
                    </span>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      title={t("profiles.rename")}
                      onClick={() => startRename(profile.id, profile.name)}
                    >
                      <Pencil className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      title={t("profiles.delete")}
                      onClick={() =>
                        setConfirm({
                          id: profile.id,
                          name: profile.name,
                        })
                      }
                    >
                      <Trash2 className="h-3.5 w-3.5 text-destructive" />
                    </Button>
                  </>
                )}
              </div>
            ))}
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => {
                cancelRename();
                onClose();
              }}
            >
              {t("common.close")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {confirm && (
        <ConfirmDialog
          isOpen
          title={t("profiles.deleteConfirmTitle")}
          message={t("profiles.deleteConfirmMessage", { name: confirm.name })}
          variant="destructive"
          onConfirm={handleConfirm}
          onCancel={() => setConfirm(null)}
        />
      )}
    </>
  );
}
