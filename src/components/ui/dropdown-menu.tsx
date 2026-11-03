import * as React from "react";
import * as DropdownMenuPrimitive from "@radix-ui/react-dropdown-menu";
import { cn } from "@/lib/utils";
import {
  finishOverlayAnimation,
  OVERLAY_MOTION_CLASS,
  type OverlayAnimationGuard,
  useOverlayAnimationGuard,
  useOverlayAnimationRef,
} from "./overlay-animation-guard";

interface DropdownMenuAnimationContextValue extends OverlayAnimationGuard {
  beginTriggerTransition: () => void;
  triggerRef: React.MutableRefObject<HTMLButtonElement | null>;
}

const DropdownMenuAnimationContext = React.createContext<
  DropdownMenuAnimationContextValue | undefined
>(undefined);

interface DropdownMenuProps
  extends React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.Root> {
  /** Overrides the OS preference when the app exposes its own motion setting. */
  reducedMotion?: boolean;
}

function DropdownMenu({
  reducedMotion: reducedMotionOverride,
  onOpenChange,
  ...props
}: DropdownMenuProps) {
  const animation = useOverlayAnimationGuard(reducedMotionOverride);
  const triggerChangeRef = React.useRef(false);
  const triggerRef = React.useRef<HTMLButtonElement | null>(null);
  const context = React.useMemo<DropdownMenuAnimationContextValue>(
    () => ({
      ...animation,
      triggerRef,
      beginTriggerTransition: () => {
        triggerChangeRef.current = true;
        animation.beginTransition();
      },
    }),
    [animation],
  );
  const handleOpenChange = React.useCallback(
    (open: boolean) => {
      if (triggerChangeRef.current) triggerChangeRef.current = false;
      else animation.endTransition();
      onOpenChange?.(open);
    },
    [animation, onOpenChange],
  );

  return (
    <DropdownMenuAnimationContext.Provider value={context}>
      <DropdownMenuPrimitive.Root {...props} onOpenChange={handleOpenChange} />
    </DropdownMenuAnimationContext.Provider>
  );
}

const DropdownMenuTrigger = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.Trigger>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.Trigger>
>(
  (
    { className, onPointerDown, onKeyDown, disabled = false, ...props },
    forwardedRef,
  ) => {
    const animation = React.useContext(DropdownMenuAnimationContext);
    const setRefs = React.useCallback(
      (node: HTMLButtonElement | null) => {
        if (animation) animation.triggerRef.current = node;
        if (typeof forwardedRef === "function") forwardedRef(node);
        else if (forwardedRef) forwardedRef.current = node;
      },
      [animation, forwardedRef],
    );
    return (
      <DropdownMenuPrimitive.Trigger
        {...props}
        // Modal menus disable pointer events on body while open. Opt the trigger
        // back in so a real second click still targets it (rather than <html>)
        // and can be rejected by the animation guard below.
        className={cn("data-[state=open]:pointer-events-auto", className)}
        disabled={disabled}
        ref={setRefs}
        onPointerDown={(event) => {
          onPointerDown?.(event);
          if (!event.defaultPrevented && animation?.isTransitioning()) {
            event.preventDefault();
          } else if (
            !event.defaultPrevented &&
            !disabled &&
            event.button === 0 &&
            event.ctrlKey === false
          ) {
            animation?.beginTriggerTransition();
          }
        }}
        onKeyDown={(event) => {
          onKeyDown?.(event);
          if (!event.defaultPrevented && animation?.isTransitioning()) {
            event.preventDefault();
          } else if (
            !event.defaultPrevented &&
            !disabled &&
            ["Enter", " ", "ArrowDown"].includes(event.key)
          ) {
            animation?.beginTriggerTransition();
          }
        }}
      />
    );
  },
);
DropdownMenuTrigger.displayName = DropdownMenuPrimitive.Trigger.displayName;

const DropdownMenuGroup = DropdownMenuPrimitive.Group;

const DropdownMenuPortal = DropdownMenuPrimitive.Portal;

const DropdownMenuSub = DropdownMenuPrimitive.Sub;

const DropdownMenuRadioGroup = DropdownMenuPrimitive.RadioGroup;

const DropdownMenuSubTrigger = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.SubTrigger>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.SubTrigger> & {
    inset?: boolean;
  }
>(({ className, inset, ...props }, ref) => (
  <DropdownMenuPrimitive.SubTrigger
    ref={ref}
    className={cn(
      "flex cursor-default select-none items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none focus:bg-subtle data-[state=open]:bg-subtle",
      inset && "pl-8",
      className,
    )}
    {...props}
  />
));
DropdownMenuSubTrigger.displayName =
  DropdownMenuPrimitive.SubTrigger.displayName;

const DropdownMenuSubContent = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.SubContent>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.SubContent>
>(({ className, ...props }, ref) => {
  const animation = React.useContext(DropdownMenuAnimationContext);
  return (
    <DropdownMenuPrimitive.SubContent
      ref={ref}
      className={cn(
        "z-[100] min-w-[8rem] rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md",
        !animation?.reducedMotion && OVERLAY_MOTION_CLASS,
        className,
      )}
      {...props}
    />
  );
});
DropdownMenuSubContent.displayName =
  DropdownMenuPrimitive.SubContent.displayName;

const DropdownMenuContent = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.Content>
>(
  (
    {
      className,
      sideOffset = 4,
      onAnimationEnd,
      onPointerDownOutside,
      ...props
    },
    forwardedRef,
  ) => {
    const animation = React.useContext(DropdownMenuAnimationContext);
    const setRefs = useOverlayAnimationRef(animation, forwardedRef);
    return (
      <DropdownMenuPrimitive.Portal>
        <DropdownMenuPrimitive.Content
          ref={setRefs}
          sideOffset={sideOffset}
          className={cn(
            "z-[100] min-w-[8rem] overflow-hidden rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md",
            !animation?.reducedMotion && OVERLAY_MOTION_CLASS,
            className,
          )}
          onAnimationEnd={(event) => {
            onAnimationEnd?.(event);
            finishOverlayAnimation(animation, event);
          }}
          onPointerDownOutside={(event) => {
            onPointerDownOutside?.(event);
            const target = event.target;
            if (
              !event.defaultPrevented &&
              animation?.isTransitioning() &&
              target instanceof Node &&
              animation.triggerRef.current?.contains(target)
            ) {
              // DismissableLayer observes pointerdown at document level after
              // the trigger handler, so the trigger's preventDefault alone
              // cannot stop this outside-dismiss path.
              event.preventDefault();
            }
          }}
          {...props}
        />
      </DropdownMenuPrimitive.Portal>
    );
  },
);
DropdownMenuContent.displayName = DropdownMenuPrimitive.Content.displayName;

const DropdownMenuItem = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.Item>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.Item> & {
    inset?: boolean;
  }
>(({ className, inset, ...props }, ref) => (
  <DropdownMenuPrimitive.Item
    ref={ref}
    className={cn(
      "relative flex cursor-default select-none items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none transition-colors focus:bg-subtle focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50",
      inset && "pl-8",
      className,
    )}
    {...props}
  />
));
DropdownMenuItem.displayName = DropdownMenuPrimitive.Item.displayName;

const DropdownMenuCheckboxItem = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.CheckboxItem>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.CheckboxItem>
>(({ className, children, checked, ...props }, ref) => (
  <DropdownMenuPrimitive.CheckboxItem
    ref={ref}
    className={cn(
      "relative flex cursor-default select-none items-center gap-2 rounded-sm py-1.5 pl-8 pr-2 text-sm outline-none transition-colors focus:bg-subtle focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50",
      className,
    )}
    checked={checked}
    {...props}
  >
    <span className="absolute left-2 flex h-3.5 w-3.5 items-center justify-center">
      <DropdownMenuPrimitive.ItemIndicator>
        <svg
          className="h-4 w-4"
          viewBox="0 0 20 20"
          fill="none"
          xmlns="http://www.w3.org/2000/svg"
        >
          <path
            d="M16.704 5.292a1 1 0 0 1 .083 1.32l-.083.094-8 8a1 1 0 0 1-1.32.083l-.094-.083-4-4a1 1 0 0 1 1.32-1.497l.094.083L8 12.585l7.293-7.292a1 1 0 0 1 1.32-.083l.094.083Z"
            fill="currentColor"
          />
        </svg>
      </DropdownMenuPrimitive.ItemIndicator>
    </span>
    {children}
  </DropdownMenuPrimitive.CheckboxItem>
));
DropdownMenuCheckboxItem.displayName =
  DropdownMenuPrimitive.CheckboxItem.displayName;

const DropdownMenuRadioItem = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.RadioItem>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.RadioItem>
>(({ className, children, ...props }, ref) => (
  <DropdownMenuPrimitive.RadioItem
    ref={ref}
    className={cn(
      "relative flex cursor-default select-none items-center gap-2 rounded-sm py-1.5 pl-2 pr-8 text-sm outline-none transition-colors focus:bg-subtle focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50",
      className,
    )}
    {...props}
  >
    <span className="absolute left-2 flex h-3.5 w-3.5 items-center justify-center">
      <DropdownMenuPrimitive.ItemIndicator>
        <div className="h-2 w-2 rounded-full bg-current" />
      </DropdownMenuPrimitive.ItemIndicator>
    </span>
    {children}
  </DropdownMenuPrimitive.RadioItem>
));
DropdownMenuRadioItem.displayName = DropdownMenuPrimitive.RadioItem.displayName;

const DropdownMenuLabel = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.Label>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.Label> & {
    inset?: boolean;
  }
>(({ className, inset, ...props }, ref) => (
  <DropdownMenuPrimitive.Label
    ref={ref}
    className={cn(
      "px-2 py-1.5 text-sm font-semibold text-fg-2",
      inset && "pl-8",
      className,
    )}
    {...props}
  />
));
DropdownMenuLabel.displayName = DropdownMenuPrimitive.Label.displayName;

const DropdownMenuSeparator = React.forwardRef<
  React.ElementRef<typeof DropdownMenuPrimitive.Separator>,
  React.ComponentPropsWithoutRef<typeof DropdownMenuPrimitive.Separator>
>(({ className, ...props }, ref) => (
  <DropdownMenuPrimitive.Separator
    ref={ref}
    className={cn("-mx-1 my-1 h-px bg-border", className)}
    {...props}
  />
));
DropdownMenuSeparator.displayName = DropdownMenuPrimitive.Separator.displayName;

const DropdownMenuShortcut = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLSpanElement>) => (
  <span
    className={cn("ml-auto text-xs tracking-widest text-fg-2", className)}
    {...props}
  />
);
DropdownMenuShortcut.displayName = "DropdownMenuShortcut";

export {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuCheckboxItem,
  DropdownMenuRadioItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuGroup,
  DropdownMenuPortal,
  DropdownMenuSub,
  DropdownMenuSubTrigger,
  DropdownMenuSubContent,
  DropdownMenuRadioGroup,
};
