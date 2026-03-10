import { useEffect, useRef, useState, type RefObject } from "react";

/**
 * Detects whether the container's children overflow the available width
 * and returns a `compact` flag for the AppSwitcher.
 *
 * Uses ResizeObserver on a flex-constrained container. The container
 * must have `flex-1 min-w-0 overflow-hidden` so its width is determined
 * by the parent layout, not its own content — avoiding the oscillation
 * problem when toggling compact mode.
 */
export function useAutoCompact(
  containerRef: RefObject<HTMLDivElement | null>,
): boolean {
  const [compact, setCompact] = useState(false);
  const normalWidthRef = useRef(0);
  const lockUntilRef = useRef(0);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const ro = new ResizeObserver(() => {
      // During expand animation, ignore resize events to prevent flicker
      if (Date.now() < lockUntilRef.current) return;

      if (!compact) {
        // Cache the total content width in normal mode
        normalWidthRef.current = el.scrollWidth;
        // Overflow detected → switch to compact
        if (el.scrollWidth > el.clientWidth + 1) {
          setCompact(true);
        }
      } else if (normalWidthRef.current > 0) {
        // In compact mode: only recover to normal if
        // available space >= what normal mode needed
        if (el.clientWidth >= normalWidthRef.current) {
          // Lock out resize events during the expand animation (200ms + 50ms margin)
          lockUntilRef.current = Date.now() + 250;
          setCompact(false);
        }
      }
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [compact]);

  return compact;
}
