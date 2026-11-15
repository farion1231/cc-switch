import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useRefreshLocalSkillMetadata } from "@/hooks/useSkills";

const refreshLocalMetadataMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api/skills", () => ({
  skillsApi: {
    refreshLocalMetadata: refreshLocalMetadataMock,
  },
}));

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("useRefreshLocalSkillMetadata", () => {
  beforeEach(() => {
    refreshLocalMetadataMock.mockReset();
  });

  it("refreshes the installed list only when metadata changed", async () => {
    refreshLocalMetadataMock.mockResolvedValue(2);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockResolvedValue(undefined);

    const { result } = renderHook(() => useRefreshLocalSkillMetadata(), {
      wrapper: createWrapper(queryClient),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(refreshLocalMetadataMock).toHaveBeenCalledTimes(1);
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "installed"],
    });
  });

  it("does not invalidate the list when nothing changed", async () => {
    refreshLocalMetadataMock.mockResolvedValue(0);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockResolvedValue(undefined);

    const { result } = renderHook(() => useRefreshLocalSkillMetadata(), {
      wrapper: createWrapper(queryClient),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(invalidateSpy).not.toHaveBeenCalled();
  });

  it("does not rescan when the installed list is invalidated", async () => {
    refreshLocalMetadataMock.mockResolvedValue(1);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue(undefined);

    const { result } = renderHook(() => useRefreshLocalSkillMetadata(), {
      wrapper: createWrapper(queryClient),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(refreshLocalMetadataMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      await queryClient.invalidateQueries({
        queryKey: ["skills", "installed"],
      });
    });
    expect(refreshLocalMetadataMock).toHaveBeenCalledTimes(1);
  });

  it("keeps the result cached across remounts in the same session", async () => {
    refreshLocalMetadataMock.mockResolvedValue(1);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue(undefined);

    const first = renderHook(() => useRefreshLocalSkillMetadata(), {
      wrapper: createWrapper(queryClient),
    });
    await waitFor(() => expect(first.result.current.isSuccess).toBe(true));
    first.unmount();

    const second = renderHook(() => useRefreshLocalSkillMetadata(), {
      wrapper: createWrapper(queryClient),
    });
    await waitFor(() => expect(second.result.current.isSuccess).toBe(true));
    expect(refreshLocalMetadataMock).toHaveBeenCalledTimes(1);
  });
});
