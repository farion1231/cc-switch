import * as React from "react";

const TRANSITION_LOCK_MS = 200;

export const OVERLAY_MOTION_CLASS =
  "data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2";

export interface OverlayAnimationGuard {
  isTransitioning: () => boolean;
  beginTransition: () => void;
}

export function useOverlayAnimationGuard(): OverlayAnimationGuard {
  const transitioningRef = React.useRef(false);
  const timerRef = React.useRef<number>();

  const clearTransition = React.useCallback(() => {
    transitioningRef.current = false;
    if (timerRef.current !== undefined) {
      window.clearTimeout(timerRef.current);
      timerRef.current = undefined;
    }
  }, []);

  const beginTransition = React.useCallback(() => {
    clearTransition();
    transitioningRef.current = true;
    timerRef.current = window.setTimeout(clearTransition, TRANSITION_LOCK_MS);
  }, [clearTransition]);

  React.useEffect(() => clearTransition, [clearTransition]);

  return React.useMemo(
    () => ({
      isTransitioning: () => transitioningRef.current,
      beginTransition,
    }),
    [beginTransition],
  );
}
