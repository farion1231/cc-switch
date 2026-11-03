import * as React from "react";
import * as PopoverPrimitive from "@radix-ui/react-popover";
import { cn } from "@/lib/utils";
import {
  finishOverlayAnimation,
  OVERLAY_MOTION_CLASS,
  type OverlayAnimationGuard,
  useOverlayAnimationGuard,
  useOverlayAnimationRef,
} from "./overlay-animation-guard";

const PopoverAnimationContext = React.createContext<
  OverlayAnimationGuard | undefined
>(undefined);

export interface PopoverProps
  extends React.ComponentPropsWithoutRef<typeof PopoverPrimitive.Root> {
  /** Overrides the OS preference when the app exposes its own motion setting. */
  reducedMotion?: boolean;
}

function Popover({
  reducedMotion: reducedMotionOverride,
  onOpenChange,
  ...props
}: PopoverProps) {
  const animation = useOverlayAnimationGuard(reducedMotionOverride);

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
          // Radix skips its toggle handler when the user handler prevents it.
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
>(
  (
    { className, align = "start", sideOffset = 4, onAnimationEnd, ...props },
    forwardedRef,
  ) => {
    const animation = React.useContext(PopoverAnimationContext);
    const setRefs = useOverlayAnimationRef(animation, forwardedRef);

    return (
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          ref={setRefs}
          align={align}
          sideOffset={sideOffset}
          className={cn(
            "z-[100] rounded-md border bg-popover text-popover-foreground shadow-md outline-none",
            !animation?.reducedMotion && OVERLAY_MOTION_CLASS,
            className,
          )}
          onAnimationEnd={(event) => {
            onAnimationEnd?.(event);
            finishOverlayAnimation(animation, event);
          }}
          {...props}
        />
      </PopoverPrimitive.Portal>
    );
  },
);
PopoverContent.displayName = PopoverPrimitive.Content.displayName;

export { Popover, PopoverTrigger, PopoverContent, PopoverAnchor };
