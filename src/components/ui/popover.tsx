import * as React from "react";
import * as PopoverPrimitive from "@radix-ui/react-popover";
import { cn } from "@/lib/utils";
import {
  OVERLAY_MOTION_CLASS,
  type OverlayAnimationGuard,
  useOverlayAnimationGuard,
} from "./overlay-animation-guard";

const PopoverAnimationContext = React.createContext<
  OverlayAnimationGuard | undefined
>(undefined);

function Popover({
  onOpenChange,
  ...props
}: React.ComponentPropsWithoutRef<typeof PopoverPrimitive.Root>) {
  const animation = useOverlayAnimationGuard();

  const handleOpenChange = React.useCallback(
    (open: boolean) => {
      animation.beginTransition();
      onOpenChange?.(open);
    },
    [animation, onOpenChange],
  );

  return (
    <PopoverAnimationContext.Provider value={animation}>
      <PopoverPrimitive.Root {...props} onOpenChange={handleOpenChange} />
    </PopoverAnimationContext.Provider>
  );
}

const PopoverTrigger = React.forwardRef<
  React.ComponentRef<typeof PopoverPrimitive.Trigger>,
  React.ComponentPropsWithoutRef<typeof PopoverPrimitive.Trigger>
>(({ onClick, ...props }, ref) => {
  const animation = React.useContext(PopoverAnimationContext);

  return (
    <PopoverPrimitive.Trigger
      {...props}
      ref={ref}
      onClick={(event) => {
        onClick?.(event);
        if (!event.defaultPrevented && animation?.isTransitioning()) {
          // 用户处理函数阻止默认行为后，Radix 不会执行切换操作。
          event.preventDefault();
        }
      }}
    />
  );
});
PopoverTrigger.displayName = PopoverPrimitive.Trigger.displayName;

const PopoverAnchor = PopoverPrimitive.Anchor;

const PopoverContent = React.forwardRef<
  React.ComponentRef<typeof PopoverPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof PopoverPrimitive.Content>
>(({ className, align = "start", sideOffset = 4, ...props }, forwardedRef) => {
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Content
        ref={forwardedRef}
        align={align}
        sideOffset={sideOffset}
        className={cn(
          "z-[100] rounded-md border bg-popover text-popover-foreground shadow-md outline-none",
          OVERLAY_MOTION_CLASS,
          className,
        )}
        {...props}
      />
    </PopoverPrimitive.Portal>
  );
});
PopoverContent.displayName = PopoverPrimitive.Content.displayName;

export { Popover, PopoverTrigger, PopoverContent, PopoverAnchor };
