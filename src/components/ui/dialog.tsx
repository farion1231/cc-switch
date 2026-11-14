import * as React from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { ModalDragStrip } from "@/components/shell/AppPageHeader";
import { cn } from "@/lib/utils";

const Dialog = DialogPrimitive.Root;

const DialogTrigger = DialogPrimitive.Trigger;

const DialogPortal = DialogPrimitive.Portal;

const DialogClose = DialogPrimitive.Close;

const DialogOverlay = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Overlay>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Overlay> & {
    zIndex?: "base" | "nested" | "alert" | "top";
  }
>(({ className, zIndex = "base", ...props }, ref) => {
  const zIndexMap = {
    base: "z-40",
    nested: "z-50",
    alert: "z-[60]",
    top: "z-[110]",
  };

  return (
    <DialogPrimitive.Overlay
      ref={ref}
      className={cn(
        "fixed inset-0 bg-overlay data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0",
        zIndexMap[zIndex],
        className,
      )}
      {...props}
    >
      <ModalDragStrip />
    </DialogPrimitive.Overlay>
  );
});
DialogOverlay.displayName = DialogPrimitive.Overlay.displayName;

const DialogContent = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content> & {
    zIndex?: "base" | "nested" | "alert" | "top";
  }
>(({ className, children, zIndex = "base", ...props }, ref) => {
  const zIndexMap = {
    base: "z-40",
    nested: "z-50",
    alert: "z-[60]",
    top: "z-[110]",
  };

  return (
    <DialogPortal>
      <DialogOverlay zIndex={zIndex} />
      <DialogPrimitive.Content
        ref={ref}
        className={cn(
          // 用 inset-0 + m-auto 居中，不用 translate(-50%)：内容高度为奇数时 translate 会落在半像素上，
          // 1x 屏（非 Retina 外接显示器）上 WebKit 按半像素重采样，整个弹窗的文字和描边都发虚（#9238）
          // h-fit 下可滚动的正文要用 flex-auto + min-h-0，别用 flex-1：WebKit 会把 flex-basis: 0% 的正文按 0 高计入，
          // 弹窗被压扁（Chromium 内核的 WebView2 不会，所以只有 Windows 看着正常）
          "fixed inset-0 m-auto flex h-fit flex-col w-full max-w-lg max-h-[90vh] border border-border bg-surface text-fg-1 shadow-v7-lg duration-200 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 rounded-dialog",
          zIndexMap[zIndex],
          className,
        )}
        onInteractOutside={(e) => {
          // 防止点击遮罩层关闭对话框
          e.preventDefault();
        }}
        {...props}
      >
        {children}
      </DialogPrimitive.Content>
    </DialogPortal>
  );
});
DialogContent.displayName = DialogPrimitive.Content.displayName;

const DialogHeader = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn(
      "flex flex-shrink-0 flex-col gap-1 px-6 pb-3 pt-5 text-start",
      className,
    )}
    {...props}
  />
);
DialogHeader.displayName = "DialogHeader";

const DialogFooter = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn(
      "flex flex-shrink-0 flex-row items-center justify-end gap-2 px-6 pb-5 pt-3",
      className,
    )}
    {...props}
  />
);
DialogFooter.displayName = "DialogFooter";

const DialogTitle = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Title>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Title>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Title
    ref={ref}
    className={cn("text-title font-semibold text-fg-1", className)}
    {...props}
  />
));
DialogTitle.displayName = DialogPrimitive.Title.displayName;

const DialogDescription = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Description>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Description>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Description
    ref={ref}
    className={cn("text-sm text-fg-2", className)}
    {...props}
  />
));
DialogDescription.displayName = DialogPrimitive.Description.displayName;

export {
  Dialog,
  DialogTrigger,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
  DialogDescription,
  DialogClose,
};
