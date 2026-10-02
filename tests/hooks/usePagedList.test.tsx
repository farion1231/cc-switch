import { describe, expect, it } from "vitest";
import { act } from "react";
import { renderHook } from "@testing-library/react";
import { usePagedList } from "@/hooks/usePagedList";

const many = (count: number) => Array.from({ length: count }, (_, i) => i);

describe("usePagedList", () => {
  it("renders only the first page and reports the rest as available", () => {
    const { result } = renderHook(() => usePagedList(many(100), 20, "fixed"));

    expect(result.current.page).toHaveLength(20);
    expect(result.current.total).toBe(100);
    expect(result.current.hasMore).toBe(true);
  });

  it("grows the page without touching the total", () => {
    const { result } = renderHook(() => usePagedList(many(100), 20, "fixed"));

    act(() => result.current.showMore());

    expect(result.current.page).toHaveLength(40);
    expect(result.current.total).toBe(100);
    expect(result.current.hasMore).toBe(true);
  });

  it("stops reporting more once every item is rendered", () => {
    const { result } = renderHook(() => usePagedList(many(25), 20, "fixed"));

    act(() => result.current.showMore());

    expect(result.current.page).toHaveLength(25);
    expect(result.current.hasMore).toBe(false);
  });

  it("never over-renders a short list", () => {
    const { result } = renderHook(() => usePagedList(many(3), 20, "fixed"));

    expect(result.current.page).toHaveLength(3);
    expect(result.current.hasMore).toBe(false);
  });

  it("returns to the first page when the filter signature changes", () => {
    // A page number that points past the new result set would show nothing.
    const { result, rerender } = renderHook(
      ({ key }) => usePagedList(many(100), 20, key),
      { initialProps: { key: "all" } },
    );

    act(() => result.current.showMore());
    expect(result.current.page).toHaveLength(40);

    rerender({ key: "search:codex" });

    expect(result.current.page).toHaveLength(20);
    expect(result.current.hasMore).toBe(true);
  });

  it("keeps the page when unrelated state re-renders the caller", () => {
    const { result, rerender } = renderHook(
      ({ key }) => usePagedList(many(100), 20, key),
      { initialProps: { key: "same" } },
    );

    act(() => result.current.showMore());
    rerender({ key: "same" });

    expect(result.current.page).toHaveLength(40);
  });

  it("resets on demand", () => {
    const { result } = renderHook(() => usePagedList(many(100), 20, "fixed"));

    act(() => result.current.showMore());
    act(() => result.current.reset());

    expect(result.current.page).toHaveLength(20);
  });

  it("reflects a list that shrinks under the same filter", () => {
    const { result, rerender } = renderHook(
      ({ items }) => usePagedList(items, 20, "same"),
      { initialProps: { items: many(100) } },
    );

    act(() => result.current.showMore());
    expect(result.current.page).toHaveLength(40);

    rerender({ items: many(10) });

    // slice() clamps, so a shorter list never renders empty slots.
    expect(result.current.page).toHaveLength(10);
    expect(result.current.hasMore).toBe(false);
  });
});
