import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import {
  focusManager,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  useInstalledSkillContents,
  useMigrateSkillStorage,
} from "@/hooks/useSkills";

const apiMocks = vi.hoisted(() => ({
  getInstalledContents: vi.fn(),
  migrateStorage: vi.fn(),
}));

vi.mock("@/lib/api/skills", () => ({
  skillsApi: {
    getInstalledContents: (...args: unknown[]) =>
      apiMocks.getInstalledContents(...args),
    migrateStorage: (...args: unknown[]) => apiMocks.migrateStorage(...args),
  },
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  return { queryClient, wrapper, invalidateSpy };
}

beforeEach(() => {
  apiMocks.getInstalledContents.mockReset();
  apiMocks.migrateStorage.mockReset().mockResolvedValue({
    migratedCount: 2,
    skippedCount: 0,
    errors: [],
  });
});

afterEach(() => {
  focusManager.setFocused(undefined);
});

describe("useInstalledSkillContents", () => {
  it("refetches stale file contents when the window regains focus", async () => {
    apiMocks.getInstalledContents
      .mockResolvedValueOnce({ skill: "old body" })
      .mockResolvedValueOnce({ skill: "new body" });
    focusManager.setFocused(false);
    const { queryClient, wrapper } = createWrapper();
    const { result } = renderHook(() => useInstalledSkillContents(), {
      wrapper,
    });

    await waitFor(() =>
      expect(result.current.data).toEqual({ skill: "old body" }),
    );

    const query = queryClient.getQueryCache().find({
      queryKey: ["skills", "installed-contents"],
    });
    act(() => {
      query?.setState({
        ...query.state,
        dataUpdatedAt: Date.now() - 31 * 1000,
      });
      focusManager.setFocused(true);
    });

    await waitFor(() =>
      expect(result.current.data).toEqual({ skill: "new body" }),
    );
    expect(apiMocks.getInstalledContents).toHaveBeenCalledTimes(2);
  });
});

describe("useMigrateSkillStorage", () => {
  it("invalidates installed Skill contents after migration", async () => {
    const { wrapper, invalidateSpy } = createWrapper();
    const { result } = renderHook(() => useMigrateSkillStorage(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync("unified");
    });

    expect(apiMocks.migrateStorage).toHaveBeenCalledWith("unified");
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "installed-contents"],
    });
  });
});
