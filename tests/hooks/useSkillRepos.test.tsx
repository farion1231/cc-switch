import type { PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useAddSkillRepo } from "@/hooks/useSkills";
import { skillsApi, type SkillRepo } from "@/lib/api/skills";

describe("useAddSkillRepo", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("updates the repository cache immediately after the repository is saved", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    const existingRepo: SkillRepo = {
      owner: "anthropics",
      name: "skills",
      branch: "main",
      enabled: true,
    };
    const previousBranch: SkillRepo = {
      owner: "owner",
      name: "repo",
      branch: "dev",
      enabled: true,
    };
    const savedRepo: SkillRepo = {
      owner: "owner",
      name: "repo",
      branch: "main",
      enabled: true,
    };
    queryClient.setQueryData<SkillRepo[]>(
      ["skills", "repos"],
      [existingRepo, previousBranch],
    );
    vi.spyOn(skillsApi, "addRepo").mockResolvedValue(true);

    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useAddSkillRepo(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync(savedRepo);
    });

    expect(queryClient.getQueryData<SkillRepo[]>(["skills", "repos"])).toEqual([
      existingRepo,
      savedRepo,
    ]);
  });
});
