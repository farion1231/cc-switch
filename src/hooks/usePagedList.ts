import { useCallback, useEffect, useMemo, useState } from "react";

export interface PagedList<T> {
  /** The slice currently rendered. Grows by `pageSize` as `showMore` is called. */
  page: T[];
  /** Everything that matches, regardless of how much is rendered. */
  total: number;
  hasMore: boolean;
  showMore: () => void;
  reset: () => void;
}

/**
 * Windowing for long local lists.
 *
 * Management panels can hold hundreds to thousands of rows (sessions, skills,
 * logs). Mounting all of them makes the first paint cost O(n) DOM nodes even
 * when the viewport shows a couple dozen, so each extra row is pure latency.
 *
 * `filterKey` is the caller's filter signature: whenever it changes (a new
 * search term, a new filter selection) the page resets to `pageSize`, because a
 * page number that points past the new result set is meaningless. It is
 * compared by value, and must be cheap to build - a joined string of the active
 * filters is the usual shape.
 */
export function usePagedList<T>(
  items: T[],
  pageSize: number,
  filterKey: string,
): PagedList<T> {
  const [visibleCount, setVisibleCount] = useState(pageSize);

  const reset = useCallback(() => setVisibleCount(pageSize), [pageSize]);

  useEffect(() => {
    setVisibleCount(pageSize);
  }, [filterKey, pageSize]);

  const page = useMemo(
    () => items.slice(0, visibleCount),
    [items, visibleCount],
  );

  const showMore = useCallback(() => {
    setVisibleCount((count) => count + pageSize);
  }, [pageSize]);

  return {
    page,
    total: items.length,
    hasMore: items.length > visibleCount,
    showMore,
    reset,
  };
}
