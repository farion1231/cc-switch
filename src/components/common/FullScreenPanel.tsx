import React from "react";
import { createPortal } from "react-dom";
import { motion, AnimatePresence, useReducedMotion } from "framer-motion";
import { ArrowLeft } from "lucide-react";
import { useTranslation } from "react-i18next";
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
  /** Entry/exit motion. Nested navigation panels can opt into a horizontal transition. */
  motionPreset?: "fade" | "slide-from-right";
  /**
   * 覆盖内容区滚动容器的内边距/间距类。默认 `px-6 py-6 space-y-6`。
   * 通过 `cn`(twMerge) 合并，传入如 `pt-3` 只覆盖顶部内边距，其余保持默认。
   */
  contentClassName?: string;
}

const DRAG_BAR_HEIGHT = isWindows() || isLinux() ? 0 : 28; // px - match App.tsx
const HEADER_HEIGHT = 64; // px - match App.tsx

let bodyScrollLockCount = 0;
let bodyOverflowBeforeFirstLock: string | null = null;

const lockBodyScroll = () => {
  if (bodyScrollLockCount === 0) {
    bodyOverflowBeforeFirstLock = document.body.style.overflow;
    document.body.style.overflow = "hidden";
  }
  bodyScrollLockCount += 1;
};

const dismissListeners = new Set<() => void>();

/**
 * 订阅「所有全屏面板都已关闭」。
 *
 * 只在计数归零时通知，所以嵌套面板（例如添加供应商里的认证设置）关闭回到
 * 上一层面板时不会触发 —— 只有真正回到页面才会。
 */
export const subscribeToFullScreenPanelDismiss = (listener: () => void) => {
  dismissListeners.add(listener);
  return () => {
    dismissListeners.delete(listener);
  };
};

const unlockBodyScroll = () => {
  bodyScrollLockCount = Math.max(0, bodyScrollLockCount - 1);
  if (bodyScrollLockCount === 0) {
    document.body.style.overflow = bodyOverflowBeforeFirstLock ?? "";
    bodyOverflowBeforeFirstLock = null;
    dismissListeners.forEach((listener) => listener());
  }
};

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
  contentClassName,
  motionPreset = "fade",
}) => {
  const { t } = useTranslation();
  const prefersReducedMotion = useReducedMotion();
  const shouldSlideFromRight =
    motionPreset === "slide-from-right" && !prefersReducedMotion;
  // 表面是不透明的整屏涂层，它本身不能做淡入淡出（见 FullScreenPanel.test.tsx）：
  // 让不透明涂层从 opacity 0 淡入，整个过渡期间都会和它下面的页面混在一起重影。
  // 表面瞬时出现，只让内部内容淡入。
  const animateContent = !shouldSlideFromRight && !prefersReducedMotion;

  React.useEffect(() => {
    if (!isOpen) return;

    lockBodyScroll();
    return unlockBodyScroll;
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
          initial={shouldSlideFromRight ? { x: "100%" } : false}
          animate={shouldSlideFromRight ? { x: 0 } : undefined}
          exit={shouldSlideFromRight ? { x: "100%" } : undefined}
          transition={
            shouldSlideFromRight
              ? { duration: 0.26, ease: [0.22, 1, 0.36, 1] }
              : undefined
          }
          className="fixed inset-0 z-[60] flex flex-col"
          style={{ backgroundColor: "hsl(var(--background))" }}
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

          <motion.div
            className="flex flex-col flex-1 min-h-0"
            initial={animateContent ? { opacity: 0, y: 10 } : false}
            animate={animateContent ? { opacity: 1, y: 0 } : undefined}
            transition={{ duration: animateContent ? 0.3 : 0 }}
          >
            {/* Header - match App.tsx */}
            <div
              className="flex-shrink-0 flex items-center"
              {...DRAG_REGION_ATTR}
              style={
                {
                  ...DRAG_REGION_STYLE,
                  backgroundColor: "hsl(var(--background))",
                  height: HEADER_HEIGHT,
                } as React.CSSProperties
              }
            >
              <div
                className="px-6 w-full flex items-center gap-4"
                {...DRAG_REGION_ATTR}
                style={{ ...DRAG_REGION_STYLE } as React.CSSProperties}
              >
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  onClick={onClose}
                  aria-label={t("common.back")}
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
              <div
                className={cn("px-6 py-6 space-y-6 w-full", contentClassName)}
              >
                {children}
              </div>
            </div>

            {/* Footer */}
            {footer && (
              <div
                className="flex-shrink-0 py-4 border-t border-border-default"
                style={{ backgroundColor: "hsl(var(--background))" }}
              >
                <div className="px-6 flex items-center justify-end gap-3">
                  {footer}
                </div>
              </div>
            )}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
};
