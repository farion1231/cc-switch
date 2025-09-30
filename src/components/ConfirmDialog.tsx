import React from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

interface ConfirmDialogProps {
  isOpen: boolean;
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  onConfirm: () => void;
  onCancel: () => void;
}

export const ConfirmDialog: React.FC<ConfirmDialogProps> = ({
  isOpen,
  title,
  message,
  confirmText,
  cancelText,
  onConfirm,
  onCancel,
}) => {
  const { t } = useTranslation();

  return (
    <Dialog open={isOpen} onOpenChange={onCancel}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 bg-red-100 dark:bg-red-500/10 rounded-full flex items-center justify-center">
              <AlertTriangle size={20} className="text-red-500" />
            </div>
            <DialogTitle>{title}</DialogTitle>
          </div>
        </DialogHeader>

        <div className="py-4">
          <p className="text-muted-foreground leading-relaxed">
            {message}
          </p>
        </div>

        <div className="flex items-center justify-end gap-3 pt-4 border-t">
          <Button variant="outline" onClick={onCancel} autoFocus>
            {cancelText || t("common.cancel")}
          </Button>
          <Button variant="destructive" onClick={onConfirm}>
            {confirmText || t("common.confirm")}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
};
