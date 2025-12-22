import React from "react";
import { createPortal } from "react-dom";
import { motion } from "framer-motion";
import { ArrowLeft } from "lucide-react";
import { Button } from "@/components/ui/button";

interface FullScreenPanelProps {
  isOpen: boolean;
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  footer?: React.ReactNode;
}

/**
 * Reusable full-screen panel component
 * Handles portal rendering, header with back button, and footer
 * Uses solid theme colors without transparency
 */
export const FullScreenPanel: React.FC<FullScreenPanelProps> = ({
  isOpen,
  title,
  onClose,
  children,
  footer,
}) => {
  React.useEffect(() => {
    if (isOpen) {
      document.body.style.overflow = "hidden";
    }
    return () => {
      document.body.style.overflow = "";
    };
  }, [isOpen]);

  if (!isOpen) return null;

  return createPortal(
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.2 }}
      className="fixed inset-0 z-[60] flex flex-col"
      style={{ backgroundColor: "hsl(var(--background))" }}
    >
      {/* Header */}
      <div
        className="flex-shrink-0 py-3 border-b border-border-default"
        style={{ backgroundColor: "hsl(var(--background))" }}
      >
        <div className="h-4 w-full" data-tauri-drag-region />
        <div className="mx-auto max-w-[56rem] px-6 flex items-center gap-4">
          <Button type="button" variant="outline" size="icon" onClick={onClose}>
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <h2 className="text-lg font-semibold text-foreground">{title}</h2>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto scroll-overlay">
        <div className="mx-auto max-w-[56rem] px-6 py-6 space-y-6 w-full">
          {children}
        </div>
      </div>

      {/* Footer */}
      {footer && (
        <div
          className="flex-shrink-0 py-4 border-t border-border-default"
          style={{ backgroundColor: "hsl(var(--background))" }}
        >
          <div className="mx-auto max-w-[56rem] px-6 flex items-center justify-end gap-3">
            {footer}
          </div>
        </div>
      )}
    </motion.div>,
    document.body,
  );
};
