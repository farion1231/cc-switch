import { act, renderHook } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useColumnVisibility } from "@/hooks/useColumnVisibility";

const STORAGE_KEY = "test.columnVisibility.logs";
const OTHER_STORAGE_KEY = "test.columnVisibility.providers";
const columns = [
  { id: "time", required: true },
  { id: "model" },
  { id: "firstToken" },
] as const;

describe("useColumnVisibility", () => {
  beforeEach(() => {
    localStorage.removeItem(STORAGE_KEY);
    localStorage.removeItem(OTHER_STORAGE_KEY);
  });

  afterEach(() => vi.restoreAllMocks());

  it("shows all columns by default and restores a hidden column after remount", () => {
    const { result, unmount } = renderHook(() =>
      useColumnVisibility(STORAGE_KEY, columns),
    );

    expect(result.current.visibility).toEqual({
      time: true,
      model: true,
      firstToken: true,
    });

    act(() => result.current.setVisible("firstToken", false));
    expect(result.current.visibility.firstToken).toBe(false);
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toEqual([
      "firstToken",
    ]);

    unmount();
    const restored = renderHook(() =>
      useColumnVisibility(STORAGE_KEY, columns),
    );
    expect(restored.result.current.visibility.firstToken).toBe(false);
    expect(restored.result.current.visibility.model).toBe(true);
  });

  it("ignores unknown and required IDs while retaining valid hidden columns", () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify(["time", "removedColumn", "model", "model"]),
    );
    const { result } = renderHook(() =>
      useColumnVisibility(STORAGE_KEY, columns),
    );

    expect(result.current.visibility).toEqual({
      time: true,
      model: false,
      firstToken: true,
    });
    act(() => result.current.setVisible("time", false));
    expect(result.current.visibility.time).toBe(true);
    act(() => result.current.setVisible("firstToken", false));
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toEqual([
      "model",
      "firstToken",
    ]);
  });

  it.each(["broken JSON", "null", "{}", '"model"', '["model", 1]'])(
    "defaults to all visible for invalid stored structure: %s",
    (stored) => {
      localStorage.setItem(STORAGE_KEY, stored);
      const { result } = renderHook(() =>
        useColumnVisibility(STORAGE_KEY, columns),
      );
      expect(result.current.visibility).toEqual({
        time: true,
        model: true,
        firstToken: true,
      });
    },
  );

  it("keeps table choices independent and resets only the chosen table", () => {
    const logs = renderHook(() => useColumnVisibility(STORAGE_KEY, columns));
    const providers = renderHook(() =>
      useColumnVisibility(OTHER_STORAGE_KEY, columns),
    );

    act(() => {
      logs.result.current.setVisible("model", false);
      providers.result.current.setVisible("firstToken", false);
    });
    act(() => logs.result.current.reset());

    expect(logs.result.current.visibility.model).toBe(true);
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toEqual([]);
    expect(providers.result.current.visibility.firstToken).toBe(false);
    expect(JSON.parse(localStorage.getItem(OTHER_STORAGE_KEY)!)).toEqual([
      "firstToken",
    ]);
  });

  it("preserves consecutive choices in one update batch", () => {
    const { result } = renderHook(() =>
      useColumnVisibility(STORAGE_KEY, columns),
    );

    act(() => {
      result.current.setVisible("model", false);
      result.current.setVisible("firstToken", false);
    });
    expect(result.current.visibility).toEqual({
      time: true,
      model: false,
      firstToken: false,
    });
    act(() => result.current.setVisible("model", true));
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toEqual([
      "firstToken",
    ]);
  });

  it("keeps newly introduced columns visible", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(["model"]));
    const { result, rerender } = renderHook(
      ({ available }) => useColumnVisibility(STORAGE_KEY, available),
      { initialProps: { available: columns.slice(0, 2) } },
    );

    rerender({ available: [...columns] });
    expect(result.current.visibility.firstToken).toBe(true);
    expect(result.current.visibility.model).toBe(false);
  });

  it("falls back on read errors and applies choices despite write errors", () => {
    const read = vi
      .spyOn(Storage.prototype, "getItem")
      .mockImplementation(() => {
        throw new Error("unavailable storage");
      });
    const write = vi
      .spyOn(Storage.prototype, "setItem")
      .mockImplementation(() => {
        throw new Error("full storage");
      });
    const { result } = renderHook(() =>
      useColumnVisibility(STORAGE_KEY, columns),
    );

    expect(result.current.visibility.model).toBe(true);
    expect(read).toHaveBeenCalledWith(STORAGE_KEY);
    act(() => result.current.setVisible("model", false));
    expect(result.current.visibility.model).toBe(false);
    act(() => result.current.reset());
    expect(result.current.visibility.model).toBe(true);
    expect(write).toHaveBeenCalledTimes(2);
  });

  it("writes only user changes once in Strict Mode", () => {
    const write = vi.spyOn(Storage.prototype, "setItem");
    const { result } = renderHook(
      () => useColumnVisibility(STORAGE_KEY, columns),
      {
        wrapper: StrictMode,
      },
    );
    expect(write).not.toHaveBeenCalled();

    act(() => result.current.setVisible("model", false));
    expect(write).toHaveBeenCalledOnce();
    expect(write).toHaveBeenCalledWith(STORAGE_KEY, '["model"]');
  });
});
