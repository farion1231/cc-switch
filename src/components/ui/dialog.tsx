import * as React from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
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
        "fixed inset-0 bg-[hsl(24_30%_8%/0.35)] backdrop-blur-[6px] data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 dark:bg-[hsl(20_40%_2%/0.6)]",
        zIndexMap[zIndex],
        className,
      )}
      {...props}
    />
  );
});
DialogOverlay.displayName = DialogPrimitive.Overlay.displayName;

const DialogContent = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content> & {
    zIndex?: "base" | "nested" | "alert" | "top";
    variant?: "default" | "fullscreen";
    overlayClassName?: string;
  }
>(
  (
    {
      className,
      children,
      zIndex = "base",
      variant = "default",
      overlayClassName,
      ...props
    },
    ref,
  ) => {
    const zIndexMap = {
      base: "z-40",
      nested: "z-50",
      alert: "z-[60]",
      top: "z-[110]",
    };

    const variantClass = {
      default:
        "fixed left-1/2 top-1/2 flex w-full max-w-lg max-h-[90vh] translate-x-[-50%] translate-y-[-50%] flex-col overflow-hidden rounded-2xl border border-border/80 bg-card text-foreground shadow-[0_0_0_1px_hsl(var(--shadow-tint)/0.03),0_8px_24px_hsl(var(--shadow-tint)/0.1),0_24px_64px_hsl(var(--shadow-tint)/0.14)] dark:shadow-[0_0_0_1px_rgba(255,255,255,0.03),0_8px_24px_hsl(var(--shadow-tint)/0.4),0_24px_64px_hsl(var(--shadow-tint)/0.5)] duration-200 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[state=closed]:slide-out-to-left-1/2 data-[state=closed]:slide-out-to-top-[48%] data-[state=open]:slide-in-from-left-1/2 data-[state=open]:slide-in-from-top-[48%]",
      fullscreen:
        "fixed inset-0 flex h-screen w-screen translate-x-0 translate-y-0 flex-col bg-transparent p-0 text-foreground shadow-none sm:rounded-none",
    }[variant];

    return (
      <DialogPortal>
        <DialogOverlay zIndex={zIndex} className={overlayClassName} />
        <DialogPrimitive.Content
          ref={ref}
          className={cn(variantClass, zIndexMap[zIndex], className)}
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
  },
);
DialogContent.displayName = DialogPrimitive.Content.displayName;

const DialogHeader = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn(
      "flex flex-shrink-0 flex-col space-y-1.5 border-b border-border bg-muted/30 px-6 py-5 text-center sm:text-left",
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
      "flex flex-shrink-0 flex-col-reverse gap-2 border-t border-border bg-muted/30 px-6 py-5 sm:flex-row sm:items-center sm:justify-end",
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
    className={cn(
      "font-display text-lg font-semibold leading-tight tracking-[-0.02em]",
      className,
    )}
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
    className={cn("text-sm text-muted-foreground", className)}
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
