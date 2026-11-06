import { renderHook, act } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";

describe("useDebouncedValue", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("returns initial value immediately", () => {
    const { result } = renderHook(() => useDebouncedValue("initial", 300));
    expect(result.current).toBe("initial");
  });

  it("updates debounced value only after specified delay", () => {
    const { result, rerender } = renderHook(
      ({ val, delay }: { val: string; delay: number }) =>
        useDebouncedValue(val, delay),
      {
        initialProps: { val: "first", delay: 300 },
      },
    );

    expect(result.current).toBe("first");

    rerender({ val: "second", delay: 300 });
    expect(result.current).toBe("first");

    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(result.current).toBe("first");

    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(result.current).toBe("second");
  });

  it("collapses multiple rapid updates into the latest one", () => {
    const { result, rerender } = renderHook(
      ({ val }: { val: string }) => useDebouncedValue(val, 300),
      {
        initialProps: { val: "a" },
      },
    );

    rerender({ val: "b" });
    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(result.current).toBe("a");

    rerender({ val: "c" });
    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(result.current).toBe("a");

    rerender({ val: "final" });
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(result.current).toBe("final");
  });

  it("updates immediately when immediateIf condition is met", () => {
    const { result, rerender } = renderHook(
      ({ val }: { val: string }) =>
        useDebouncedValue(val, 300, {
          immediateIf: (v) => !v.trim(),
        }),
      {
        initialProps: { val: "searching" },
      },
    );

    expect(result.current).toBe("searching");

    // Clearing input triggers immediate update
    rerender({ val: "" });
    expect(result.current).toBe("");

    // Normal typing still debounces
    rerender({ val: "new search" });
    expect(result.current).toBe("");

    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(result.current).toBe("new search");
  });

  it("cleans up timer on unmount", () => {
    const { unmount, rerender } = renderHook(
      ({ val }: { val: string }) => useDebouncedValue(val, 300),
      {
        initialProps: { val: "hello" },
      },
    );

    rerender({ val: "world" });
    unmount();

    expect(() => {
      vi.advanceTimersByTime(300);
    }).not.toThrow();
  });
});
