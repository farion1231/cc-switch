import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Plus, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { useBudgetStatuses, useDeleteBudget, useUpdateBudget } from "@/lib/query/budget";
import type { TokenBudget } from "@/types/budget";
import { BudgetCard } from "./BudgetCard";
import { BudgetEditor } from "./BudgetEditor";
import { BudgetDeleteDialog } from "./BudgetDeleteDialog";

export function BudgetList() {
  const { t } = useTranslation();
  const { data: statuses, isLoading } = useBudgetStatuses();
  const deleteMutation = useDeleteBudget();
  const updateMutation = useUpdateBudget();

  // Editor state
  const [editorOpen, setEditorOpen] = useState(false);
  const [editingBudget, setEditingBudget] = useState<TokenBudget | undefined>();

  // Delete dialog state
  const [deleteTarget, setDeleteTarget] = useState<TokenBudget | null>(null);

  const handleEdit = (budget: TokenBudget) => {
    setEditingBudget(budget);
    setEditorOpen(true);
  };

  const handleCreate = () => {
    setEditingBudget(undefined);
    setEditorOpen(true);
  };

  const handleToggleEnabled = (budget: TokenBudget, enabled: boolean) => {
    updateMutation.mutate(
      { id: budget.id, patch: { enabled } },
      {
        onSuccess: () => toast.success(t("budget.saved")),
        onError: (e) => toast.error(String(e)),
      },
    );
  };

  const handleDelete = () => {
    if (!deleteTarget) return;
    deleteMutation.mutate(deleteTarget.id, {
      onSuccess: () => {
        toast.success(t("budget.deleted"));
        setDeleteTarget(null);
      },
      onError: (e) => toast.error(String(e)),
    });
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {/* 添加按钮 */}
      <div className="flex justify-end">
        <Button onClick={handleCreate} size="sm">
          <Plus className="h-4 w-4 mr-1.5" />
          {t("budget.add")}
        </Button>
      </div>

      {/* 预算卡片列表 */}
      {statuses && statuses.length > 0 ? (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {statuses.map((status) => (
            <BudgetCard
              key={status.budget.id}
              status={status}
              onEdit={() => handleEdit(status.budget)}
              onDelete={() => setDeleteTarget(status.budget)}
              onToggleEnabled={(enabled) =>
                handleToggleEnabled(status.budget, enabled)
              }
            />
          ))}
        </div>
      ) : (
        <div className="text-center py-12 text-muted-foreground text-sm">
          {t("budget.noBudgets")}
        </div>
      )}

      {/* 编辑/新建弹窗 */}
      <BudgetEditor
        open={editorOpen}
        onOpenChange={setEditorOpen}
        budget={editingBudget}
      />

      {/* 删除确认弹窗 */}
      <BudgetDeleteDialog
        isOpen={!!deleteTarget}
        budgetName={deleteTarget?.name ?? ""}
        onConfirm={handleDelete}
        onCancel={() => setDeleteTarget(null)}
      />
    </div>
  );
}
