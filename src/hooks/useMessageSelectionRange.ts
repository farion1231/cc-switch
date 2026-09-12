import { useCallback, useEffect, useState, type RefObject } from "react";
import { defaultRangeExtractor, type Range } from "@tanstack/react-virtual";

// Keep native Selection endpoints and the intervening messages mounted while scrolling.
export function useMessageSelectionRange(
  containerRef: RefObject<HTMLDivElement>,
  sessionKey: string | null,
) {
  const [selectedRange, setSelectedRange] = useState<{
    start: number;
    end: number;
  } | null>(null);

  useEffect(() => {
    setSelectedRange(null);
    const updateSelection = () => {
      const selection = document.getSelection();
      const container = containerRef.current;
      if (
        !container ||
        !selection ||
        selection.isCollapsed ||
        !container.contains(selection.anchorNode) ||
        !container.contains(selection.focusNode)
      ) {
        setSelectedRange(null);
        return;
      }
      const getMessage = (node: Node | null) =>
        (node instanceof Element
          ? node
          : node?.parentElement
        )?.closest<HTMLElement>("[data-index]");
      const anchor = getMessage(selection.anchorNode);
      const focus = getMessage(selection.focusNode);
      if (!anchor || !focus) {
        setSelectedRange(null);
        return;
      }
      const start = Math.min(
        Number(anchor.dataset.index),
        Number(focus.dataset.index),
      );
      const end = Math.max(
        Number(anchor.dataset.index),
        Number(focus.dataset.index),
      );
      setSelectedRange((previous) =>
        previous?.start === start && previous.end === end
          ? previous
          : { start, end },
      );
    };
    document.addEventListener("selectionchange", updateSelection);
    return () =>
      document.removeEventListener("selectionchange", updateSelection);
  }, [containerRef, sessionKey]);

  return useCallback(
    (range: Range) => {
      const visible = defaultRangeExtractor(range);
      if (!selectedRange || visible.length === 0) return visible;
      const start = Math.min(visible[0], selectedRange.start);
      const end = Math.min(
        range.count - 1,
        Math.max(visible[visible.length - 1], selectedRange.end),
      );
      return Array.from(
        { length: end - start + 1 },
        (_, index) => start + index,
      );
    },
    [selectedRange],
  );
}
