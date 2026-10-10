import React from "react";
import { createPortal } from "react-dom";
import { motion, AnimatePresence, useReducedMotion } from "framer-motion";
import { ChevronLeft } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { isTextEditableTarget } from "@/utils/domUtils";
import { useUnsavedChangesTracker } from "@/lib/unsavedChanges";
import { cn } from "@/lib/utils";

interface FullScreenPanelProps {
  isOpen: boolean;
  title: string;
  /** 标题后的灰字（应用名） */
  subtitle?: React.ReactNode;
  /** 返回按钮的读屏文字，默认「返回」 */
  backLabel?: string;
  /** 页头右侧 */
  actions?: React.ReactNode;
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
  /**
   * 编辑表单：用户改过内容后，从侧栏、托盘、⌘, 离开当前页前会先确认。
   * 只给编辑页打开；授权中心、测速这类页面不需要。
   */
  trackUnsavedChanges?: boolean;
}

/** 外壳里的内容区（App 的 <main id="content-area">）：二级页只盖住它，侧栏留着 */
const CONTENT_AREA_ID = "content-area";

let bodyScrollLockCount = 0;
let bodyOverflowBeforeFirstLock: string | null = null;

const lockBodyScroll = () => {
  if (bodyScrollLockCount === 0) {
    bodyOverflowBeforeFirstLock = document.body.style.overflow;
    document.body.style.overflow = "hidden";
  }
  bodyScrollLockCount += 1;
};

const unlockBodyScroll = () => {
  bodyScrollLockCount = Math.max(0, bodyScrollLockCount - 1);
  if (bodyScrollLockCount === 0) {
    document.body.style.overflow = bodyOverflowBeforeFirstLock ?? "";
    bodyOverflowBeforeFirstLock = null;
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
  subtitle,
  backLabel,
  actions,
  onClose,
  children,
  footer,
  contentClassName,
  motionPreset = "fade",
  trackUnsavedChanges = false,
}) => {
  // 面板关掉后组件可能还挂着：只在打开时登记，关掉即撤销
  const unsavedChangesHandlers = useUnsavedChangesTracker(
    trackUnsavedChanges && isOpen,
  );
  const { t } = useTranslation();
  const prefersReducedMotion = useReducedMotion();
  const shouldSlideFromRight =
    motionPreset === "slide-from-right" && !prefersReducedMotion;

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

  const host =
    typeof document !== "undefined"
      ? document.getElementById(CONTENT_AREA_ID)
      : null;

  return createPortal(
    <AnimatePresence>
      {isOpen && (
        <motion.div
          initial={
            prefersReducedMotion
              ? false
              : shouldSlideFromRight
                ? { x: "100%" }
                : { opacity: 0 }
          }
          animate={shouldSlideFromRight ? { x: 0 } : { opacity: 1 }}
          exit={shouldSlideFromRight ? { x: "100%" } : { opacity: 0 }}
          transition={
            shouldSlideFromRight
              ? { duration: 0.26, ease: [0.22, 1, 0.36, 1] }
              : { duration: prefersReducedMotion ? 0 : 0.2 }
          }
          className={cn(
            "inset-0 z-[60] flex flex-col bg-app",
            host ? "absolute" : "fixed",
          )}
        >
          <AppPageHeader
            variant="app"
            title={title}
            // 标题里常带用户起的名字，不限长度；必须能收缩截断，否则会把窗口按钮挤出可视区
            truncateTitle
            subtitle={subtitle}
            actions={actions}
            leading={
              <HoverTip content={backLabel ?? t("common.back")}>
                <Button
                  type="button"
                  variant="quiet"
                  size="icon-compact"
                  className="h-8 w-8"
                  onClick={onClose}
                  aria-label={backLabel ?? t("common.back")}
                >
                  <ChevronLeft className="h-4 w-4" strokeWidth={1.5} />
                </Button>
              </HoverTip>
            }
          />

          <div className="flex-1 overflow-y-auto scroll-stable">
            <div
              className={cn("w-full space-y-6 px-6 py-6", contentClassName)}
              {...unsavedChangesHandlers}
            >
              {children}
            </div>
          </div>

          {footer && (
            <div className="flex h-14 shrink-0 items-center justify-end gap-2 border-t border-border px-6">
              {footer}
            </div>
          )}
        </motion.div>
      )}
    </AnimatePresence>,
    host ?? document.body,
  );
};
