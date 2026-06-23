import type { ReactNode } from "react";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useDiscoverableSkills } from "@/hooks/useSkills";
import type { DiscoverableSkill, SkillRepo } from "@/lib/api/skills";

const apiMocks = vi.hoisted(() => ({
  discoverAvailable: vi.fn(),
  loadCachedDiscoverable: vi.fn(),
}));

vi.mock("@/lib/api/skills", () => ({
  skillsApi: {
    discoverAvailable: (...args: unknown[]) =>
      apiMocks.discoverAvailable(...args),
    loadCachedDiscoverable: (...args: unknown[]) =>
      apiMocks.loadCachedDiscoverable(...args),
  },
}));

function makeSkill(
  name: string,
  overrides: Partial<DiscoverableSkill> = {},
): DiscoverableSkill {
  return {
    key: `owner/repo:${name}`,
    name,
    description: `${name} description`,
    directory: name,
    repoOwner: "owner",
    repoName: "repo",
    repoBranch: "main",
    ...overrides,
  };
}

function makeRepo(overrides: Partial<SkillRepo> = {}): SkillRepo {
  return {
    owner: "owner",
    name: "repo",
    branch: "main",
    enabled: true,
    ...overrides,
  };
}

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  return { queryClient, wrapper };
}

beforeEach(() => {
  apiMocks.discoverAvailable.mockReset();
  apiMocks.loadCachedDiscoverable.mockReset();
});

describe("useDiscoverableSkills", () => {
  it("shows persisted discovery cache while remote refresh is still running", async () => {
    const cached = [makeSkill("cached-skill")];
    const fresh = [makeSkill("fresh-skill")];
    let resolveRemote: (value: DiscoverableSkill[]) => void = () => {};
    const remotePromise = new Promise<DiscoverableSkill[]>((resolve) => {
      resolveRemote = resolve;
    });
    apiMocks.loadCachedDiscoverable.mockResolvedValue(cached);
    apiMocks.discoverAvailable.mockReturnValue(remotePromise);

    const { queryClient, wrapper } = createWrapper();
    const { result } = renderHook(() => useDiscoverableSkills(), { wrapper });

    await waitFor(() => {
      expect(queryClient.getQueryData(["skills", "discoverable"])).toEqual(
        cached,
      );
    });
    expect(result.current.data).toEqual(cached);

    resolveRemote(fresh);

    await waitFor(() => {
      expect(result.current.data).toEqual(fresh);
    });
  });

  it("drops stale remote results for repositories removed during refresh", async () => {
    const fresh = [
      makeSkill("kept"),
      makeSkill("deleted", {
        repoOwner: "deleted-owner",
        repoName: "deleted-repo",
      }),
    ];
    apiMocks.loadCachedDiscoverable.mockResolvedValue([]);
    apiMocks.discoverAvailable.mockResolvedValue(fresh);

    const { queryClient, wrapper } = createWrapper();
    queryClient.setQueryData(["skills", "repos"], [makeRepo()]);

    const { result } = renderHook(() => useDiscoverableSkills(), { wrapper });

    await waitFor(() => {
      expect(result.current.data?.map((skill) => skill.name)).toEqual(["kept"]);
    });
  });
});
