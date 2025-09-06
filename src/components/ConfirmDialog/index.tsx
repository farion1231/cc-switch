import React from "react";
import "./index.css";

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
  confirmText = "确定",
  cancelText = "取消",
  onConfirm,
  onCancel,
}) => {
  if (!isOpen) return null;

  return (
    <div className="confirm-overlay">
      <div className="confirm-dialog">
        <div className="confirm-header">
          <h3>{title}</h3>
        </div>
        <div className="confirm-content">
          <p>{message}</p>
        </div>
        <div className="confirm-actions">
          <button
            className="confirm-btn cancel-btn"
            onClick={onCancel}
            autoFocus
          >
            {cancelText}
          </button>
          <button
            className="confirm-btn confirm-btn-primary"
            onClick={onConfirm}
          >
            {confirmText}
          </button>
        </div>
      </div>
    </div>
  );
};
