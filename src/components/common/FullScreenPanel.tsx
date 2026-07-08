import React from "react";
import { createPortal } from "react-dom";
import { motion, AnimatePresence } from "framer-motion";
import { ArrowLeft } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  isWindows,
  isLinux,
  DRAG_REGION_ATTR,
  DRAG_REGION_STYLE,
} from "@/lib/platform";
import { isTextEditableTarget } from "@/utils/domUtils";
import { cn } from "@/lib/utils";

interface FullScreenPanelProps {
  isOpen: boolean;
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  footer?: React.ReactNode;
  /**
   * 覆盖内容区滚动容器的内边距/间距类。默认 `px-6 py-6 space-y-6`。
   * 通过 `cn`(twMerge) 合并，传入如 `pt-3` 只覆盖顶部内边距，其余保持默认。
   */
  contentClassName?: string;
}

const DRAG_BAR_HEIGHT = isWindows() || isLinux() ? 0 : 28; // px - match App.tsx
const HEADER_HEIGHT = 72; // px - match App.tsx

/**
 * Reusable full-screen panel component
 * Handles portal rendering, header with back button, and footer
 * Uses the shared low-cost liquid glass language from the main shell
 */
export const FullScreenPanel: React.FC<FullScreenPanelProps> = ({
  isOpen,
  title,
  onClose,
  children,
  footer,
  contentClassName,
}) => {
  React.useEffect(() => {
    if (isOpen) {
      document.body.style.overflow = "hidden";
    }
    return () => {
      document.body.style.overflow = "";
    };
  }, [isOpen]);

  // ESC 键关闭面板
  const onCloseRef = React.useRef(onClose);

  React.useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  React.useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        // 子组件（例如 Radix 的 Select/Dialog/Dropdown）如果已经消费了 ESC，就不要再关闭整个面板
        if (event.defaultPrevented) {
          return;
        }

        if (isTextEditableTarget(event.target)) {
          return; // 让输入框自己处理 ESC（比如清空、失焦等）
        }

        event.stopPropagation(); // 阻止事件继续冒泡到 window，避免触发 App.tsx 的全局监听
        onCloseRef.current();
      }
    };

    // 使用冒泡阶段监听，让子组件（如 Radix UI）优先处理 ESC
    window.addEventListener("keydown", handleKeyDown, false);
    return () => {
      window.removeEventListener("keydown", handleKeyDown, false);
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
          className="fixed inset-0 z-[60] flex flex-col bg-[radial-gradient(circle_at_top,rgba(10,132,255,0.08),transparent_28%),linear-gradient(180deg,hsl(var(--background))/0.94_0%,hsl(var(--background))/0.98_100%)] backdrop-blur-md"
          style={{ backgroundColor: "transparent" }}
        >
          {/* Drag region - match App.tsx. Linux 上 DRAG_BAR_HEIGHT=0，
              直接跳过整个元素；macOS 保留 28px 拖拽占位。 */}
          {DRAG_BAR_HEIGHT > 0 && (
            <div
              data-tauri-drag-region
              style={
                {
                  WebkitAppRegion: "drag",
                  height: DRAG_BAR_HEIGHT,
                } as React.CSSProperties
              }
            />
          )}

          {/* Header - match App.tsx */}
          <div
            className="flex flex-shrink-0 items-center"
            {...DRAG_REGION_ATTR}
            style={
              {
                ...DRAG_REGION_STYLE,
                height: HEADER_HEIGHT,
              } as React.CSSProperties
            }
          >
            <div
              className="mx-4 my-2 flex h-[calc(100%-16px)] w-full items-center"
              {...DRAG_REGION_ATTR}
              style={{ ...DRAG_REGION_STYLE } as React.CSSProperties}
            >
              <div className="toolbar-cluster flex h-full w-full items-center gap-4 rounded-[1rem] border-border/70 bg-card/80 px-5 shadow-[0_14px_34px_rgba(15,23,42,0.07)] backdrop-blur-xl">
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  onClick={onClose}
                  className="select-none border-border/60 bg-background/75 hover:bg-accent"
                  style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
                >
                  <ArrowLeft className="h-4 w-4" />
                </Button>
                <h2 className="select-none truncate text-lg font-semibold tracking-[-0.03em] text-foreground">
                  {title}
                </h2>
              </div>
            </div>
          </div>

          {/* Content */}
          <div className="mx-auto flex w-full max-w-[1480px] flex-1 overflow-y-auto scroll-overlay px-5 md:px-6">
            <div
              className={cn(
                "w-full space-y-6 px-1 py-4 md:py-6",
                contentClassName,
              )}
            >
              {children}
            </div>
          </div>

          {/* Footer */}
          {footer && (
            <div className="flex-shrink-0">
              <div className="mx-4 my-2">
                <div className="toolbar-cluster flex items-center justify-end gap-3 rounded-[1rem] border-border/70 bg-card/82 px-5 py-3 shadow-[0_14px_34px_rgba(15,23,42,0.07)] backdrop-blur-xl">
                  {footer}
                </div>
              </div>
            </div>
          )}
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
};
