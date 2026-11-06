import type { ReactNode } from "react";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { settingsApi } from "@/lib/api";
import { useWindowControlsLayout } from "@/lib/query/queries";

describe("window controls layout query", () => {
  it("skips disabled controls and caches GTK and built-in styles separately", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const images = {
      close: {
        normal: "normal",
        hover: "hover",
        pressed: "pressed",
        width: 24,
        height: 24,
      },
    };
    const getLayout = vi
      .spyOn(settingsApi, "getWindowControlsLayout")
      .mockImplementation(async (useGtkStyle) => ({
        side: "left",
        order: ["close", "minimize", "maximize"],
        images: useGtkStyle ? images : undefined,
      }));
    function Wrapper({ children }: { children: ReactNode }) {
      return (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      );
    }
    const { result, rerender, unmount } = renderHook(
      ({ enabled, useGtkStyle }) =>
        useWindowControlsLayout(enabled, useGtkStyle),
      { wrapper: Wrapper, initialProps: { enabled: false, useGtkStyle: true } },
    );
    expect(getLayout).not.toHaveBeenCalled();
    rerender({ enabled: true, useGtkStyle: true });
    await waitFor(() => expect(result.current.data?.images).toEqual(images));
    rerender({ enabled: true, useGtkStyle: false });
    await waitFor(() => expect(getLayout).toHaveBeenCalledWith(false));
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data?.images).toBeUndefined();
    rerender({ enabled: true, useGtkStyle: true });
    await waitFor(() => expect(result.current.data?.images).toEqual(images));
    expect(getLayout).toHaveBeenCalledTimes(2);
    unmount();
    client.clear();
    getLayout.mockRestore();
  });
});
