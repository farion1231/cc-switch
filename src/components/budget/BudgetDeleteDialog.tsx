import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "@/components/ConfirmDialog";

interface BudgetDeleteDialogProps {
  isOpen: boolean;
  budgetName: string;
  onConfirm: () => void;
  onCancel: () => void;
}

export function BudgetDeleteDialog({
  isOpen,
  budgetName,
  onConfirm,
  onCancel,
}: BudgetDeleteDialogProps) {
  const { t } = useTranslation();

  return (
    <ConfirmDialog
      isOpen={isOpen}
      title={t("budget.delete")}
      message={t("budget.deleteConfirm", { name: budgetName })}
      variant="destructive"
      onConfirm={onConfirm}
      onCancel={onCancel}
    />
  );
}
