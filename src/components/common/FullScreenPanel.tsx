import React from "react";
import { createPortal } from "react-dom";
import { motion, AnimatePresence } from "framer-motion";
import { ArrowLeft } from "lucide-react";
import { Button } from "@/components/ui/button";
import { isWindows, isLinux } from "@/lib/platform";

interface FullScreenPanelProps {
  isOpen: boolean;
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  footer?: React.ReactNode;
}

const DRAG_BAR_HEIGHT = isWindows() || isLinux() ? 0 : 28; // px - match App.tsx
const HEADER_HEIGHT = 64; // px - match App.tsx

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

  return createPortal(
    <AnimatePresence>
      {isOpen && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2 }}
          className="fixed inset-0 z-[60] flex flex-col"
          style={{ backgroundColor: "hsl(var(--background))" }}
        >
          {/* Drag region - match App.tsx */}
          <div
            data-tauri-drag-region
            style={
              {
                WebkitAppRegion: "drag",
                height: DRAG_BAR_HEIGHT,
              } as React.CSSProperties
            }
          />

          {/* Header - match App.tsx */}
          <div
            className="flex-shrink-0 flex items-center"
            data-tauri-drag-region
            style={
              {
                WebkitAppRegion: "drag",
                backgroundColor: "hsl(var(--background))",
                height: HEADER_HEIGHT,
              } as React.CSSProperties
            }
          >
            <div
              className="mx-auto max-w-[56rem] px-6 w-full flex items-center gap-4"
              data-tauri-drag-region
              style={{ WebkitAppRegion: "drag" } as React.CSSProperties}
            >
              <Button
                type="button"
                variant="outline"
                size="icon"
                onClick={onClose}
                className="rounded-lg select-none"
                style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
              >
                <ArrowLeft className="h-4 w-4" />
              </Button>
              <h2 className="text-lg font-semibold text-foreground select-none">
                {title}
              </h2>
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
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
};
