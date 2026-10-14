import type { ReactNode } from "react";
import { renderHook, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi, beforeEach, afterAll } from "vitest";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import type { DragEndEvent } from "@dnd-kit/core";
import { useDragSort } from "@/hooks/useDragSort";

const updateSortOrderMock = vi.fn();
const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    updateSortOrder: (...args: unknown[]) => updateSortOrderMock(...args),
    updateTrayMenu: vi.fn().mockResolvedValue(true),
  },
}));

interface WrapperProps {
  children: ReactNode;
}

function createWrapper() {
  const queryClient = new QueryClient();

  const wrapper = ({ children }: WrapperProps) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  return { wrapper, queryClient };
}

const mockProviders: Record<string, Provider> = {
  a: {
    id: "a",
    name: "AAA",
    settingsConfig: {},
    sortIndex: 1,
    createdAt: 5,
  },
  b: {
    id: "b",
    name: "BBB",
    settingsConfig: {},
    sortIndex: 0,
    createdAt: 10,
  },
  c: {
    id: "c",
    name: "CCC",
    settingsConfig: {},
    createdAt: 1,
  },
};

function deferredSave() {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, resolve, reject };
}

const drop = (active: string, over: string) =>
  ({ active: { id: active }, over: { id: over } }) as DragEndEvent;

const orderedProviders = (...ids: string[]) =>
  Object.fromEntries(
    ids.map((id, sortIndex) => [id, { ...mockProviders[id], sortIndex }]),
  );

describe("useDragSort", () => {
  beforeEach(() => {
    updateSortOrderMock.mockReset();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    consoleErrorSpy.mockClear();
  });

  afterAll(() => {
    consoleErrorSpy.mockRestore();
  });

  it("should sort providers by sortIndex, createdAt, and name", () => {
    const { wrapper } = createWrapper();

    const { result } = renderHook(() => useDragSort(mockProviders, "claude"), {
      wrapper,
    });

    expect(result.current.sortedProviders.map((item) => item.id)).toEqual([
      "b",
      "a",
      "c",
    ]);
  });

  it("should call API and invalidate query cache after successful drag", async () => {
    updateSortOrderMock.mockResolvedValue(true);
    const { wrapper, queryClient } = createWrapper();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    const { result } = renderHook(() => useDragSort(mockProviders, "claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.handleDragEnd({
        active: { id: "b" },
        over: { id: "a" },
      } as any);
    });

    expect(updateSortOrderMock).toHaveBeenCalledTimes(1);
    expect(updateSortOrderMock).toHaveBeenCalledWith(
      [
        { id: "a", sortIndex: 0 },
        { id: "b", sortIndex: 1 },
        { id: "c", sortIndex: 2 },
      ],
      "claude",
    );
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["providers", "claude"],
    });
    expect(toastSuccessMock).toHaveBeenCalledTimes(1);
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("should show error toast when drag operation fails", async () => {
    updateSortOrderMock.mockRejectedValue(new Error("network"));
    const { wrapper } = createWrapper();

    const { result } = renderHook(() => useDragSort(mockProviders, "claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.handleDragEnd({
        active: { id: "b" },
        over: { id: "a" },
      } as any);
    });

    expect(toastErrorMock).toHaveBeenCalledTimes(1);
    expect(toastSuccessMock).not.toHaveBeenCalled();
    expect(consoleErrorSpy).toHaveBeenCalled();
  });

  it("should not trigger API call when there is no valid target", async () => {
    const { wrapper } = createWrapper();

    const { result } = renderHook(() => useDragSort(mockProviders, "claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.handleDragEnd({
        active: { id: "b" },
        over: null,
      } as any);
    });

    expect(updateSortOrderMock).not.toHaveBeenCalled();
  });

  it("keeps the dropped order visible until refreshed props confirm the save", async () => {
    const save = deferredSave();
    updateSortOrderMock.mockReturnValue(save.promise);
    const { wrapper } = createWrapper();
    const { result, rerender } = renderHook(
      ({ providers }) => useDragSort(providers, "claude"),
      { wrapper, initialProps: { providers: mockProviders } },
    );
    let saving!: Promise<void>;
    act(() => {
      saving = result.current.handleDragEnd(drop("b", "a"));
    });
    expect(
      result.current.sortedProviders.map((provider) => provider.id),
    ).toEqual(["a", "b", "c"]);

    // A refresh may update other provider details while the save is pending.
    rerender({
      providers: {
        ...mockProviders,
        a: { ...mockProviders.a, name: "Renamed" },
      },
    });
    expect(result.current.sortedProviders[0].name).toBe("Renamed");
    await act(async () => {
      save.resolve();
      await saving;
    });
    expect(
      result.current.sortedProviders.map((provider) => provider.id),
    ).toEqual(["a", "b", "c"]);

    rerender({ providers: orderedProviders("a", "b", "c") });
    // Once confirmed, future external changes are no longer pinned to this drop.
    rerender({ providers: orderedProviders("c", "b", "a") });
    expect(
      result.current.sortedProviders.map((provider) => provider.id),
    ).toEqual(["c", "b", "a"]);
  });

  it("restores the saved order when persistence fails", async () => {
    const save = deferredSave();
    updateSortOrderMock.mockReturnValue(save.promise);
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useDragSort(mockProviders, "claude"), {
      wrapper,
    });
    let saving!: Promise<void>;
    await act(async () => {
      saving = result.current.handleDragEnd(drop("b", "a"));
    });
    expect(result.current.sortedProviders[0].id).toBe("a");
    await act(async () => {
      save.reject(new Error("save failed"));
      await saving;
    });
    expect(
      result.current.sortedProviders.map((provider) => provider.id),
    ).toEqual(["b", "a", "c"]);
    expect(toastErrorMock).toHaveBeenCalledTimes(1);
  });

  it("persists rapid drops in order without letting an older response undo the latest drop", async () => {
    const first = deferredSave();
    const second = deferredSave();
    updateSortOrderMock
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const { wrapper } = createWrapper();
    const { result, rerender } = renderHook(
      ({ providers }) => useDragSort(providers, "claude"),
      { wrapper, initialProps: { providers: mockProviders } },
    );
    let savingFirst!: Promise<void>;
    let savingSecond!: Promise<void>;
    await act(async () => {
      savingFirst = result.current.handleDragEnd(drop("b", "a"));
    });
    // Undo the first drop before it finishes saving.
    await act(async () => {
      savingSecond = result.current.handleDragEnd(drop("a", "b"));
    });
    expect(updateSortOrderMock).toHaveBeenCalledTimes(1);
    expect(
      result.current.sortedProviders.map((provider) => provider.id),
    ).toEqual(["b", "a", "c"]);
    rerender({ providers: orderedProviders("a", "b", "c") });
    await act(async () => {
      first.resolve();
      await savingFirst;
    });
    expect(updateSortOrderMock).toHaveBeenCalledTimes(2);
    expect(
      updateSortOrderMock.mock.calls[1][0].map(
        (item: { id: string }) => item.id,
      ),
    ).toEqual(["b", "a", "c"]);
    expect(
      result.current.sortedProviders.map((provider) => provider.id),
    ).toEqual(["b", "a", "c"]);
    await act(async () => {
      second.resolve();
      await savingSecond;
    });
    expect(
      result.current.sortedProviders.map((provider) => provider.id),
    ).toEqual(["b", "a", "c"]);
    rerender({ providers: orderedProviders("b", "a", "c") });
  });

  it("does not carry a pending order into another app", async () => {
    const save = deferredSave();
    updateSortOrderMock.mockReturnValue(save.promise);
    const { wrapper } = createWrapper();
    const { result, rerender } = renderHook(
      ({ appId }: { appId: AppId }) => useDragSort(mockProviders, appId),
      { wrapper, initialProps: { appId: "claude" as AppId } },
    );
    let saving!: Promise<void>;
    await act(async () => {
      saving = result.current.handleDragEnd(drop("b", "a"));
    });
    rerender({ appId: "codex" });
    expect(
      result.current.sortedProviders.map((provider) => provider.id),
    ).toEqual(["b", "a", "c"]);
    await act(async () => {
      save.resolve();
      await saving;
    });
    expect(
      result.current.sortedProviders.map((provider) => provider.id),
    ).toEqual(["b", "a", "c"]);
  });
});
