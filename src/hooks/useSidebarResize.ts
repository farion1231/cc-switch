import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type RefObject,
} from "react";
import {
  SIDEBAR_EXPANDED_WIDTH,
  sidebarRailWidth,
} from "./useSidebarCollapsed";

export const SIDEBAR_MIN_WIDTH = 180;
export const SIDEBAR_MAX_WIDTH = 360;
const STORAGE_KEY = "cc-switch-sidebar-width";
const maximum = () =>
  Math.max(
    SIDEBAR_MIN_WIDTH,
    Math.min(SIDEBAR_MAX_WIDTH, window.innerWidth - 360),
  );
const clamp = (value: number) =>
  Math.min(maximum(), Math.max(SIDEBAR_MIN_WIDTH, value));
const readWidth = () => {
  try {
    const saved = Number(localStorage.getItem(STORAGE_KEY));
    if (saved > 0 && Number.isFinite(saved)) return clamp(saved);
  } catch {
    /* Storage is optional. */
  }
  return SIDEBAR_EXPANDED_WIDTH;
};

/** Pointer moves only update the nav's width once per frame; React/storage update on release. */
export function useSidebarResize(
  navRef: RefObject<HTMLElement | null>,
  collapsed: boolean,
  setCollapsed: (value: boolean) => void,
) {
  const [width, setWidth] = useState(readWidth);
  const [max, setMax] = useState(maximum);
  const drag = useRef<{
    id: number;
    target: HTMLElement;
    x: number;
    initial: number;
    next: number;
    direction: number;
    frame: number | null;
    cursor: string;
    selection: string;
  } | null>(null);

  const save = useCallback(
    (value: number) => {
      const next = clamp(value);
      setWidth(next);

      try {
        localStorage.setItem(STORAGE_KEY, String(next));
      } catch {
        /* Keep the current session usable. */
      }
    },
    [navRef],
  );

  const finish = useCallback(
    (cancel: boolean, restore = true) => {
      const current = drag.current;
      if (!current) return;
      drag.current = null;
      if (current.frame !== null) cancelAnimationFrame(current.frame);
      document.documentElement.style.cursor = current.cursor;
      document.documentElement.style.userSelect = current.selection;
      navRef.current?.removeAttribute("data-resizing");
      if (cancel) {
        if (restore && navRef.current)
          navRef.current.style.width = `${current.initial}px`;
      } else save(current.next);
      if (current.target.hasPointerCapture?.(current.id))
        current.target.releasePointerCapture(current.id);
    },
    [navRef, save],
  );

  useLayoutEffect(() => {
    // A button/shortcut toggle also ends an in-progress resize.
    finish(true, false);
  }, [collapsed, finish]);

  useEffect(() => {
    const onResize = () => {
      finish(true);
      setMax(maximum());
      setWidth((value) => clamp(value));
    };
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape" && drag.current) {
        event.preventDefault();
        finish(true);
      }
    };
    window.addEventListener("resize", onResize);
    window.addEventListener("keydown", onKey);
    return () => {
      finish(true);
      window.removeEventListener("resize", onResize);
      window.removeEventListener("keydown", onKey);
    };
  }, [finish]);

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || drag.current || !navRef.current) return;
    event.preventDefault();
    event.currentTarget.focus();
    const initial = navRef.current.getBoundingClientRect().width;
    drag.current = {
      id: event.pointerId,
      target: event.currentTarget,
      x: event.clientX,
      initial,
      next: initial,
      direction: getComputedStyle(navRef.current).direction === "rtl" ? -1 : 1,
      frame: null,
      cursor: document.documentElement.style.cursor,
      selection: document.documentElement.style.userSelect,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
    navRef.current.setAttribute("data-resizing", "true");
    document.documentElement.style.cursor = "col-resize";
    document.documentElement.style.userSelect = "none";
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const current = drag.current;
    if (!current || current.id !== event.pointerId) return;
    const requested =
      current.initial + (event.clientX - current.x) * current.direction;
    // Leave a deliberate gap below the minimum width before snapping closed.
    if (
      (!collapsed && requested <= 130) ||
      (collapsed && requested >= sidebarRailWidth() + 24)
    ) {
      finish(true, false);
      setCollapsed(!collapsed);
      return;
    }
    if (collapsed) return;
    current.next = clamp(requested);
    if (current.frame !== null) return;
    current.frame = requestAnimationFrame(() => {
      current.frame = null;
      if (navRef.current) navRef.current.style.width = `${current.next}px`;
    });
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (drag.current) return;
    const direction =
      navRef.current && getComputedStyle(navRef.current).direction === "rtl"
        ? -1
        : 1;
    let next: number;
    if (event.key === "Enter" || event.key === "Home") {
      event.preventDefault();
      setCollapsed(event.key === "Home" || !collapsed);
      return;
    }
    if (event.key === "ArrowLeft") next = width - 10 * direction;
    else if (event.key === "ArrowRight") next = width + 10 * direction;
    else if (event.key === "End") next = max;
    else return;
    event.preventDefault();
    if (collapsed && event.key !== "End") {
      if (next > width) setCollapsed(false);
      return;
    }
    if (next < SIDEBAR_MIN_WIDTH) setCollapsed(true);
    else {
      save(next);
      if (collapsed) setCollapsed(false);
    }
  };
  return {
    width,
    min: sidebarRailWidth(),
    max,
    resizeHandle: {
      onPointerDown,
      onPointerMove,
      onKeyDown,
      onPointerUp: (event: PointerEvent<HTMLDivElement>) => {
        if (drag.current?.id === event.pointerId) finish(collapsed);
      },
      onPointerCancel: () => finish(true),
      onLostPointerCapture: () => finish(true),
      onDoubleClick: () => {
        save(SIDEBAR_EXPANDED_WIDTH);
        if (collapsed) setCollapsed(false);
      },
    },
  };
}
