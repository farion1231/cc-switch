import type { ReactNode } from "react";
import type { DragEndEvent } from "@dnd-kit/core";
import { renderHook, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi, beforeEach, afterAll } from "vitest";
import type { Provider } from "@/types";
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

function dragEvent(activeId: string, overId: string): DragEndEvent {
  return {
    activatorEvent: new Event("pointerup"),
    active: {
      id: activeId,
      data: { current: {} },
      rect: { current: { initial: null, translated: null } },
    },
    over: {
      id: overId,
      data: { current: {} },
      rect: { top: 0, left: 0, bottom: 0, right: 0, width: 0, height: 0 },
      disabled: false,
    },
    collisions: null,
    delta: { x: 0, y: 0 },
  };
}

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

  it("persists only saved OMP providers when native login cards are displayed", async () => {
    updateSortOrderMock.mockResolvedValue(true);
    const { wrapper } = createWrapper();
    const native: Provider = {
      id: "openai-codex",
      name: "openai-codex",
      settingsConfig: {},
      sortIndex: -1,
      meta: { nativeLogin: true },
    };
    const { result } = renderHook(
      () => useDragSort({ ...mockProviders, [native.id]: native }, "omp"),
      { wrapper },
    );

    expect(result.current.sortedProviders.map((item) => item.id)).toEqual([
      "openai-codex",
      "b",
      "a",
      "c",
    ]);
    await act(async () => {
      await result.current.handleDragEnd(dragEvent("b", "a"));
    });

    expect(updateSortOrderMock).toHaveBeenCalledWith(
      [
        { id: "a", sortIndex: 0 },
        { id: "b", sortIndex: 1 },
        { id: "c", sortIndex: 2 },
      ],
      "omp",
    );
  });

  it.each([
    ["openai-codex", "a"],
    ["a", "openai-codex"],
  ])("ignores OMP dragging from %s to %s", async (active, over) => {
    const { wrapper } = createWrapper();
    const native: Provider = {
      id: "openai-codex",
      name: "openai-codex",
      settingsConfig: {},
      meta: { nativeLogin: true },
    };
    const { result } = renderHook(
      () => useDragSort({ ...mockProviders, [native.id]: native }, "omp"),
      { wrapper },
    );

    await act(async () => {
      await result.current.handleDragEnd(dragEvent(active, over));
    });

    expect(updateSortOrderMock).not.toHaveBeenCalled();
  });

  it.each(["pi", "hermes"] as const)(
    "preserves ordinary %s drag persistence",
    async (appId) => {
      updateSortOrderMock.mockResolvedValue(true);
      const { wrapper } = createWrapper();
      const { result } = renderHook(() => useDragSort(mockProviders, appId), {
        wrapper,
      });

      await act(async () => {
        await result.current.handleDragEnd(dragEvent("b", "a"));
      });

      expect(updateSortOrderMock).toHaveBeenCalledWith(
        [
          { id: "a", sortIndex: 0 },
          { id: "b", sortIndex: 1 },
          { id: "c", sortIndex: 2 },
        ],
        appId,
      );
    },
  );

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
});
