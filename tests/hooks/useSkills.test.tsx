import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useMigrateSkillStorage } from "@/hooks/useSkills";

const apiMocks = vi.hoisted(() => ({
  migrateStorage: vi.fn(),
}));

vi.mock("@/lib/api/skills", () => ({
  skillsApi: {
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

  return { wrapper, invalidateSpy };
}

beforeEach(() => {
  apiMocks.migrateStorage.mockReset().mockResolvedValue({
    migratedCount: 2,
    skippedCount: 0,
    errors: [],
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
