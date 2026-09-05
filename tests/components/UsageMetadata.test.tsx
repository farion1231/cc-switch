import { render, renderHook, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { PropsWithChildren } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageMetadataDisplay } from "@/components/usage/UsageMetadataDisplay";
import { useRequestLogs } from "@/lib/query/usage";
import type { LogFilters } from "@/types/usage";

const getLogs = vi.hoisted(() => vi.fn());
vi.mock("@/lib/api/usage", () => ({ usageApi: { getRequestLogs: getLogs } }));
vi.mock("react-i18next", async () => {
  const { default: en } = await import("@/i18n/locales/en.json");
  return {
    useTranslation: () => ({
      t: (key: string) =>
        key.split(".").reduce<any>((value, part) => value?.[part], en) ?? key,
    }),
  };
});

describe("usage metadata", () => {
  beforeEach(() => {
    getLogs.mockReset();
    getLogs.mockResolvedValue({ data: [], total: 0, page: 0, pageSize: 20 });
  });

  it("shows Fast, effort and the observed source, leaving missing settings unknown", () => {
    const { rerender } = render(
      <UsageMetadataDisplay
        request={{
          serviceTier: "priority",
          serviceTierSource: "request",
          reasoningEffort: "xhigh",
        }}
        showSource
      />,
    );
    expect(screen.getByText("Fast · Request setting")).toBeInTheDocument();
    expect(screen.getByText("xhigh")).toBeInTheDocument();
    rerender(
      <UsageMetadataDisplay
        request={{
          serviceTier: "default",
          serviceTierSource: "response",
          reasoningEffort: "none",
        }}
        showSource
      />,
    );
    expect(screen.getByText("Standard · Response")).toBeInTheDocument();
    expect(screen.getByText("none")).toBeInTheDocument();
    rerender(<UsageMetadataDisplay request={{}} />);
    expect(screen.getAllByText("Unknown")).toHaveLength(2);
  });

  it("refetches logs immediately when mode or effort changes with polling disabled", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Infinity } },
    });
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result, rerender, unmount } = renderHook(
      ({ filters }: { filters: LogFilters }) =>
        useRequestLogs({
          filters,
          range: { preset: "today" },
          options: { refetchInterval: false },
        }),
      { wrapper, initialProps: { filters: {} } },
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    rerender({ filters: { serviceTier: "fast" } });
    await waitFor(() =>
      expect(getLogs).toHaveBeenLastCalledWith(
        expect.objectContaining({ serviceTier: "fast" }),
        0,
        20,
      ),
    );
    rerender({ filters: { serviceTier: "fast", reasoningEffort: "ultra" } });
    await waitFor(() =>
      expect(getLogs).toHaveBeenLastCalledWith(
        expect.objectContaining({
          serviceTier: "fast",
          reasoningEffort: "ultra",
        }),
        0,
        20,
      ),
    );
    expect(getLogs).toHaveBeenCalledTimes(3);
    unmount();
    client.clear();
  });
});
