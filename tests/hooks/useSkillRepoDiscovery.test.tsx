import type { PropsWithChildren } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { replaceRepoDiscovery } from "@/hooks/useSkills.helpers";
import {
  useAddSkillRepo,
  useDiscoverableSkills,
  useRefreshDiscoverableSkills,
  useRemoveSkillRepo,
} from "@/hooks/useSkills";
import type {
  DiscoverableSkill,
  SkillDiscoveryResult,
  SkillRepo,
} from "@/lib/api/skills";

const apiMocks = vi.hoisted(() => ({
  addRepo: vi.fn(),
  removeRepo: vi.fn(),
  discoverRepo: vi.fn(),
  discoverAvailable: vi.fn(),
}));

vi.mock("@/lib/api/skills", () => ({
  skillsApi: apiMocks,
}));

const DISCOVERABLE = ["skills", "discoverable"];

function skill(owner: string, repo: string, name: string): DiscoverableSkill {
  return {
    key: `${owner}/${repo}:${name}`,
    name,
    description: "",
    directory: name,
    repoOwner: owner,
    repoName: repo,
    repoBranch: "main",
  };
}

const repoA: SkillRepo = {
  owner: "a",
  name: "one",
  branch: "main",
  enabled: true,
};

function setup() {
  const queryClient = new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
    },
  });
  const wrapper = ({ children }: PropsWithChildren) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { queryClient, wrapper };
}

describe("replaceRepoDiscovery", () => {
  it("replaces only the target repo's skills and failures, sorted by name", () => {
    const existing: SkillDiscoveryResult = {
      skills: [skill("a", "one", "old"), skill("b", "two", "zeta")],
      failures: [
        { owner: "a", name: "one", branch: "main", error: "x" },
        { owner: "b", name: "three", branch: "main", error: "y" },
      ],
    };
    const next: SkillDiscoveryResult = {
      skills: [skill("a", "one", "Alpha")],
      failures: [],
    };

    expect(replaceRepoDiscovery(existing, repoA, next)).toEqual({
      skills: [skill("a", "one", "Alpha"), skill("b", "two", "zeta")],
      failures: [{ owner: "b", name: "three", branch: "main", error: "y" }],
    });
    expect(replaceRepoDiscovery(existing, repoA)?.skills).toEqual([
      skill("b", "two", "zeta"),
    ]);
  });

  it("leaves an empty cache alone", () => {
    expect(
      replaceRepoDiscovery(undefined, repoA, { skills: [], failures: [] }),
    ).toBeUndefined();
  });
});

describe("repo mutations patch discovery instead of refetching every repo", () => {
  beforeEach(() => {
    Object.values(apiMocks).forEach((mock) => mock.mockReset());
    apiMocks.addRepo.mockResolvedValue(true);
    apiMocks.removeRepo.mockResolvedValue(true);
  });

  it("adding a repo discovers just that repo", async () => {
    const { queryClient, wrapper } = setup();
    queryClient.setQueryData<SkillDiscoveryResult>(DISCOVERABLE, {
      skills: [skill("b", "two", "beta")],
      failures: [],
    });
    const fresh = { skills: [skill("a", "one", "alpha")], failures: [] };
    apiMocks.discoverRepo.mockResolvedValue(fresh);

    const { result } = renderHook(() => useAddSkillRepo(), { wrapper });
    let returned: SkillDiscoveryResult | null = null;
    await act(async () => {
      returned = await result.current.mutateAsync(repoA);
    });

    expect(returned).toEqual(fresh);
    expect(apiMocks.discoverRepo).toHaveBeenCalledWith("a", "one");
    expect(apiMocks.discoverAvailable).not.toHaveBeenCalled();
    expect(
      queryClient
        .getQueryData<SkillDiscoveryResult>(DISCOVERABLE)
        ?.skills.map((s) => s.name),
    ).toEqual(["alpha", "beta"]);
  });

  it("disabling a repo drops its skills without any network call", async () => {
    const { queryClient, wrapper } = setup();
    queryClient.setQueryData<SkillDiscoveryResult>(DISCOVERABLE, {
      skills: [skill("a", "one", "alpha"), skill("b", "two", "beta")],
      failures: [],
    });

    const { result } = renderHook(() => useAddSkillRepo(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ ...repoA, enabled: false });
    });

    expect(apiMocks.discoverRepo).not.toHaveBeenCalled();
    expect(apiMocks.discoverAvailable).not.toHaveBeenCalled();
    expect(
      queryClient
        .getQueryData<SkillDiscoveryResult>(DISCOVERABLE)
        ?.skills.map((s) => s.name),
    ).toEqual(["beta"]);
  });

  it("removing a repo drops its skills without any network call", async () => {
    const { queryClient, wrapper } = setup();
    queryClient.setQueryData<SkillDiscoveryResult>(DISCOVERABLE, {
      skills: [skill("a", "one", "alpha"), skill("b", "two", "beta")],
      failures: [],
    });

    const { result } = renderHook(() => useRemoveSkillRepo(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ owner: "a", name: "one" });
    });

    expect(apiMocks.discoverAvailable).not.toHaveBeenCalled();
    expect(
      queryClient
        .getQueryData<SkillDiscoveryResult>(DISCOVERABLE)
        ?.skills.map((s) => s.name),
    ).toEqual(["beta"]);
  });
});

describe("repo mutation races", () => {
  beforeEach(() => {
    // 前面的用例会把发现结果写进本地缓存，它会成为 initialData
    localStorage.clear();
    Object.values(apiMocks).forEach((mock) => mock.mockReset());
    apiMocks.addRepo.mockResolvedValue(true);
    apiMocks.removeRepo.mockResolvedValue(true);
  });

  it("falls back to a full discovery when there is nothing to patch yet", async () => {
    const { queryClient, wrapper } = setup();
    // 第一次整体发现挂着不回来，第二次正常返回
    apiMocks.discoverAvailable
      .mockReturnValueOnce(new Promise(() => {}))
      .mockResolvedValue({
        skills: [skill("a", "one", "alpha")],
        failures: [],
      });
    apiMocks.discoverRepo.mockResolvedValue({
      skills: [skill("a", "one", "alpha")],
      failures: [],
    });
    renderHook(() => useDiscoverableSkills(), { wrapper });
    await vi.waitFor(() =>
      expect(apiMocks.discoverAvailable).toHaveBeenCalledTimes(1),
    );
    const { result } = renderHook(() => useAddSkillRepo(), { wrapper });

    // 首次整体发现还没回来就添加：它会被取消，必须再整体发现一次
    await act(async () => {
      await result.current.mutateAsync(repoA);
    });

    await vi.waitFor(() =>
      expect(
        queryClient.getQueryData<SkillDiscoveryResult>(DISCOVERABLE)?.skills,
      ).toEqual([skill("a", "one", "alpha")]),
    );
    expect(apiMocks.discoverAvailable).toHaveBeenCalledTimes(2);
    queryClient.clear();
  });

  it("refreshes the repo list before the repo's archive finishes downloading", async () => {
    const { queryClient, wrapper } = setup();
    apiMocks.discoverRepo.mockReturnValue(new Promise(() => {}));
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    const { result } = renderHook(() => useAddSkillRepo(), { wrapper });
    act(() => {
      void result.current.mutateAsync(repoA);
    });

    await vi.waitFor(() => expect(apiMocks.discoverRepo).toHaveBeenCalled());
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["skills", "repos"] });
  });

  it("a slow discovery does not restore a repo removed meanwhile", async () => {
    const { queryClient, wrapper } = setup();
    queryClient.setQueryData<SkillDiscoveryResult>(DISCOVERABLE, {
      skills: [skill("b", "two", "beta")],
      failures: [],
    });
    let resolveDiscover!: (value: SkillDiscoveryResult) => void;
    apiMocks.discoverRepo.mockReturnValue(
      new Promise<SkillDiscoveryResult>((resolve) => {
        resolveDiscover = resolve;
      }),
    );

    const add = renderHook(() => useAddSkillRepo(), { wrapper });
    const remove = renderHook(() => useRemoveSkillRepo(), { wrapper });
    let adding!: Promise<unknown>;
    act(() => {
      adding = add.result.current.mutateAsync(repoA);
    });
    await vi.waitFor(() => expect(apiMocks.discoverRepo).toHaveBeenCalled());
    await act(async () => {
      await remove.result.current.mutateAsync({ owner: "a", name: "one" });
    });
    await act(async () => {
      resolveDiscover({ skills: [skill("a", "one", "alpha")], failures: [] });
      await adding;
    });

    expect(
      queryClient
        .getQueryData<SkillDiscoveryResult>(DISCOVERABLE)
        ?.skills.map((s) => s.name),
    ).toEqual(["beta"]);
  });
});

describe("useRefreshDiscoverableSkills", () => {
  it("asks the backend to bypass its per-repo cache", async () => {
    apiMocks.discoverAvailable.mockResolvedValue({ skills: [], failures: [] });
    const { queryClient, wrapper } = setup();
    // 需要一个活跃的 observer，refetchQueries 才会真正发请求；先等挂载时的
    // 首次发现结束，否则刷新会直接复用那次在飞的请求
    const discover = renderHook(() => useDiscoverableSkills(), { wrapper });
    await vi.waitFor(() =>
      expect(discover.result.current.isFetching).toBe(false),
    );

    const { result } = renderHook(() => useRefreshDiscoverableSkills(), {
      wrapper,
    });
    await act(async () => {
      await result.current();
    });

    expect(apiMocks.discoverAvailable).toHaveBeenLastCalledWith(true);
    queryClient.clear();
  });
});
