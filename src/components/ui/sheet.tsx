import * as React from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { ChevronLeft, X } from "lucide-react";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { Button } from "@/components/ui/button";
import { DRAG_REGION_ATTR } from "@/lib/platform";
import { useUnsavedChangesTracker } from "@/lib/unsavedChanges";
import { cn } from "@/lib/utils";

/**
 * 右侧抽屉（v7：圆角 14、大阴影，宽度按内容 420 / 560）。基于 Radix Dialog：
 * Escape 关闭、打开时背景不可操作、关闭后焦点回到触发按钮。
 */
const Sheet = DialogPrimitive.Root;
const SheetTrigger = DialogPrimitive.Trigger;
const SheetClose = DialogPrimitive.Close;

interface SheetContentProps
  extends React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content> {
  width?: number;
  /** 右上角关闭按钮的无障碍名字 */
  closeLabel: string;
  /**
   * 点遮罩就关。默认不关、和 DialogContent 一致：抽屉大多是表单，遮罩占了窗口大半，
   * 误点一下就把草稿丢掉。只给没有草稿可丢的抽屉（只读详情、即点即存的开关）打开。
   */
  dismissOnOutsideClick?: boolean;
}

const SheetContent = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Content>,
  SheetContentProps
>(
  (
    {
      className,
      children,
      width = 420,
      closeLabel,
      style,
      dismissOnOutsideClick = false,
      onInteractOutside,
      ...props
    },
    ref,
  ) => (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-overlay data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0" />
      <DialogPrimitive.Content
        ref={ref}
        style={{ width, ...style }}
        className={cn(
          "fixed inset-y-0 end-0 z-50 flex max-w-[100vw] flex-col overflow-hidden rounded-s-dialog border-s border-border bg-surface text-fg-1 shadow-v7-lg outline-none duration-200 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:slide-out-to-right-4 data-[state=open]:slide-in-from-right-4",
          className,
        )}
        onInteractOutside={(event) => {
          onInteractOutside?.(event);
          if (!dismissOnOutsideClick) event.preventDefault();
        }}
        {...props}
      >
        {children}
        <DialogPrimitive.Close
          aria-label={closeLabel}
          className="absolute end-3 top-3 inline-flex h-7 w-7 items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1"
        >
          <X className="h-4 w-4" strokeWidth={1.5} />
        </DialogPrimitive.Close>
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  ),
);
SheetContent.displayName = "SheetContent";

/** 登记整页编辑器里的改动；`display: contents` 不影响布局 */
function UnsavedChangesScope({ children }: { children: React.ReactNode }) {
  return (
    <div className="contents" {...useUnsavedChangesTracker()}>
      {children}
    </div>
  );
}

/**
 * 整页版的 SheetContent：盖住内容区，页头带返回按钮（同 FullScreenPanel），内容随窗口铺满。
 * 配合 `<Sheet modal={false}>` 使用：侧栏和窗口拖动区照常可用；点外面不关。
 */
const SheetPageContent = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Content>,
  Omit<
    React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content>,
    "title"
  > & {
    title: React.ReactNode;
    /** 返回按钮的无障碍名字 */
    closeLabel: string;
  }
>(({ title, closeLabel, children, ...props }, ref) => (
  <DialogPrimitive.Portal
    container={document.getElementById("content-area") ?? undefined}
  >
    <DialogPrimitive.Content
      ref={ref}
      className="absolute inset-0 z-[60] flex flex-col bg-app text-fg-1 outline-none"
      onInteractOutside={(event) => event.preventDefault()}
      {...props}
    >
      <AppPageHeader
        variant="app"
        truncateTitle
        title={
          <DialogPrimitive.Title asChild>
            <span {...DRAG_REGION_ATTR}>{title}</span>
          </DialogPrimitive.Title>
        }
        leading={
          <DialogPrimitive.Close asChild>
            <Button
              variant="quiet"
              size="icon-compact"
              className="h-8 w-8"
              aria-label={closeLabel}
            >
              <ChevronLeft className="h-4 w-4" strokeWidth={1.5} />
            </Button>
          </DialogPrimitive.Close>
        }
      />
      <UnsavedChangesScope>{children}</UnsavedChangesScope>
    </DialogPrimitive.Content>
  </DialogPrimitive.Portal>
));
SheetPageContent.displayName = "SheetPageContent";

const SheetHeader = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn("shrink-0 space-y-1 pe-12 ps-5 pt-4", className)}
    {...props}
  />
);

const SheetBody = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn(
      "min-h-0 flex-1 overflow-y-auto scroll-stable overscroll-contain px-5 py-4",
      className,
    )}
    {...props}
  />
);

const SheetFooter = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn(
      "flex shrink-0 items-center justify-end gap-2 border-t border-border px-5 py-3",
      className,
    )}
    {...props}
  />
);

const SheetTitle = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Title>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Title>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Title
    ref={ref}
    className={cn("text-section", className)}
    {...props}
  />
));
SheetTitle.displayName = "SheetTitle";

const SheetDescription = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Description>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Description>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Description
    ref={ref}
    className={cn("text-caption text-fg-2", className)}
    {...props}
  />
));
SheetDescription.displayName = "SheetDescription";

export {
  Sheet,
  SheetTrigger,
  SheetClose,
  SheetContent,
  SheetPageContent,
  SheetHeader,
  SheetBody,
  SheetFooter,
  SheetTitle,
  SheetDescription,
};
