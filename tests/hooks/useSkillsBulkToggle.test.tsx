import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useBulkToggleSkillApp } from "@/hooks/useSkills";

const toggleAppMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api/skills", () => ({
  skillsApi: {
    toggleApp: toggleAppMock,
  },
}));

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("Skills bulk toggle hook", () => {
  beforeEach(() => {
    toggleAppMock.mockReset();
  });

  it("stays pending until the refreshed skill list is available", async () => {
    let releaseInvalidation: (() => void) | undefined;
    const invalidationPending = new Promise<void>((resolve) => {
      releaseInvalidation = resolve;
    });
    toggleAppMock.mockResolvedValue(undefined);
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockImplementation(() => invalidationPending);
    const { result } = renderHook(() => useBulkToggleSkillApp(), {
      wrapper: createWrapper(queryClient),
    });

    let mutation!: Promise<unknown>;
    act(() => {
      mutation = result.current.mutateAsync({
        ids: ["alpha", "beta"],
        app: "claude",
        enabled: true,
      });
    });

    await waitFor(() => expect(toggleAppMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(1));
    expect(result.current.isPending).toBe(true);

    releaseInvalidation?.();
    await act(async () => {
      await mutation;
    });

    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "installed"],
    });
    await waitFor(() => expect(result.current.isPending).toBe(false));
  });
});
