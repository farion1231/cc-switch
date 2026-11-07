import type { PropsWithChildren } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  mergeDiscoveryResults,
  replaceRepoDiscovery,
} from "@/hooks/useSkills.helpers";
import {
  useAddSkillRepo,
  useDiscoverableSkills,
  useRefreshDiscoverableSkills,
  useRemoveSkillRepo,
  usePendingSkillRepos,
  useImportSkillsFromApps,
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
  importFromApps: vi.fn(),
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
  it("preserves the target repo's last successful skills when its retry fails", () => {
    const previous = { skills: [skill("a", "one", "alpha")], failures: [] };
    const failed = {
      skills: [],
      failures: [{ owner: "a", name: "one", branch: "main", error: "timeout" }],
    };
    expect(replaceRepoDiscovery(previous, repoA, failed)).toEqual({
      skills: [skill("a", "one", "alpha")],
      failures: failed.failures,
    });
  });

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

describe("mergeDiscoveryResults", () => {
  const failure = { owner: "a", name: "one", branch: "main", error: "timeout" };

  it("keeps a resolved fallback branch when the requested branch has not changed", () => {
    const cached = {
      ...skill("a", "one", "alpha"),
      repoBranch: "master",
      repoRequestedBranch: "main",
    };
    expect(
      mergeDiscoveryResults(
        { skills: [cached], failures: [] },
        { skills: [], failures: [failure] },
      ).skills,
    ).toEqual([cached]);
  });

  it("does not reuse a fallback from a different requested branch", () => {
    const cached = {
      ...skill("a", "one", "alpha"),
      repoBranch: "main",
      repoRequestedBranch: "develop",
    };
    expect(
      mergeDiscoveryResults(
        { skills: [cached], failures: [] },
        { skills: [], failures: [failure] },
      ).skills,
    ).toEqual([]);
  });

  it("keeps only failed repositories from the previous result and uses fresh successful data", () => {
    const previous = {
      skills: [
        skill("a", "one", "alpha"),
        skill("b", "two", "old"),
        skill("c", "removed", "gone"),
      ],
      failures: [],
    };
    const next = { skills: [skill("b", "two", "beta")], failures: [failure] };
    expect(mergeDiscoveryResults(previous, next)).toEqual({
      skills: [skill("a", "one", "alpha"), skill("b", "two", "beta")],
      failures: [failure],
    });
  });

  it("clears stale skills and errors after a successful empty scan", () => {
    expect(
      mergeDiscoveryResults(
        { skills: [skill("a", "one", "alpha")], failures: [failure] },
        { skills: [], failures: [] },
      ),
    ).toEqual({ skills: [], failures: [] });
  });

  it("does not reuse another branch's results after switching branches", () => {
    expect(
      mergeDiscoveryResults(
        { skills: [skill("a", "one", "alpha")], failures: [] },
        { skills: [], failures: [{ ...failure, branch: "develop" }] },
      ).skills,
    ).toEqual([]);
  });

  it("reports first-time failures without inventing cached skills", () => {
    const failed = { skills: [], failures: [failure] };
    expect(mergeDiscoveryResults(undefined, failed)).toEqual(failed);
  });
});

describe("failed full discovery keeps its persisted successful cache", () => {
  it("retains the old count in both the query and localStorage after a background failure", async () => {
    localStorage.clear();
    const previous = { skills: [skill("a", "one", "alpha")], failures: [] };
    localStorage.setItem(
      "cc-switch-cache:skills.discoverable.v1",
      JSON.stringify(previous),
    );
    apiMocks.discoverAvailable.mockResolvedValue({
      skills: [],
      failures: [{ owner: "a", name: "one", branch: "main", error: "timeout" }],
    });
    const { queryClient, wrapper } = setup();
    const discover = renderHook(() => useDiscoverableSkills(), { wrapper });
    const refresh = renderHook(() => useRefreshDiscoverableSkills(), {
      wrapper,
    });
    await vi.waitFor(() =>
      expect(discover.result.current.isFetching).toBe(false),
    );
    await act(async () => {
      await refresh.result.current();
    });
    expect(discover.result.current.data).toEqual(previous.skills);
    expect(
      JSON.parse(
        localStorage.getItem("cc-switch-cache:skills.discoverable.v1")!,
      ).skills,
    ).toEqual(previous.skills);
    queryClient.clear();
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
      returned = (await result.current.mutateAsync(repoA)).discovery;
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
    const pending = renderHook(() => usePendingSkillRepos(), { wrapper });

    const { result } = renderHook(() => useAddSkillRepo(), { wrapper });
    act(() => {
      void result.current.mutateAsync(repoA);
    });

    await vi.waitFor(() => expect(apiMocks.discoverRepo).toHaveBeenCalled());
    await vi.waitFor(() => expect(pending.result.current).toEqual([repoA]));
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

  it.each([false, true])(
    "a newer full refresh supersedes an older single-repo scan (empty: %s)",
    async (empty) => {
      const { queryClient, wrapper } = setup();
      const initial = { skills: [skill("a", "one", "initial")], failures: [] };
      const newer = {
        skills: empty ? [] : [skill("a", "one", "new-full")],
        failures: [],
      };
      apiMocks.discoverAvailable
        .mockResolvedValueOnce(initial)
        .mockResolvedValue(newer);
      const discover = renderHook(() => useDiscoverableSkills(), { wrapper });
      await vi.waitFor(() =>
        expect(discover.result.current.data).toEqual(initial.skills),
      );
      let resolveSingle!: (value: SkillDiscoveryResult) => void;
      apiMocks.discoverRepo.mockReturnValue(
        new Promise<SkillDiscoveryResult>((resolve) => {
          resolveSingle = resolve;
        }),
      );
      const add = renderHook(() => useAddSkillRepo(), { wrapper });
      let adding!: Promise<unknown>;
      act(() => {
        adding = add.result.current.mutateAsync(repoA);
      });
      await vi.waitFor(() => expect(apiMocks.discoverRepo).toHaveBeenCalled());
      const refresh = renderHook(() => useRefreshDiscoverableSkills(), {
        wrapper,
      });
      await act(async () => {
        await refresh.result.current();
      });
      await act(async () => {
        resolveSingle({
          skills: [skill("a", "one", "older-single")],
          failures: [],
        });
        await adding;
      });
      expect(queryClient.getQueryData(DISCOVERABLE)).toEqual(newer);
      await vi.waitFor(() =>
        expect(discover.result.current.data).toEqual(newer.skills),
      );
      expect(
        localStorage.getItem("cc-switch-cache:skills.discoverable.v1"),
      ).toBe(JSON.stringify(newer));
      queryClient.clear();
    },
  );

  it("an older single-repo result does not cancel a newer full refresh still in flight", async () => {
    const { queryClient, wrapper } = setup();
    const initial = { skills: [skill("a", "one", "initial")], failures: [] };
    apiMocks.discoverAvailable.mockResolvedValueOnce(initial);
    const discover = renderHook(() => useDiscoverableSkills(), { wrapper });
    await vi.waitFor(() =>
      expect(discover.result.current.data).toEqual(initial.skills),
    );
    let resolveSingle!: (value: SkillDiscoveryResult) => void;
    let resolveFull!: (value: SkillDiscoveryResult) => void;
    apiMocks.discoverRepo.mockReturnValue(
      new Promise<SkillDiscoveryResult>((resolve) => {
        resolveSingle = resolve;
      }),
    );
    apiMocks.discoverAvailable.mockReturnValue(
      new Promise<SkillDiscoveryResult>((resolve) => {
        resolveFull = resolve;
      }),
    );
    const add = renderHook(() => useAddSkillRepo(), { wrapper });
    const refresh = renderHook(() => useRefreshDiscoverableSkills(), {
      wrapper,
    });
    let adding!: Promise<unknown>;
    let refreshing!: Promise<unknown>;
    act(() => {
      adding = add.result.current.mutateAsync(repoA);
    });
    await vi.waitFor(() => expect(apiMocks.discoverRepo).toHaveBeenCalled());
    act(() => {
      refreshing = refresh.result.current();
    });
    await vi.waitFor(() =>
      expect(apiMocks.discoverAvailable).toHaveBeenCalledTimes(2),
    );
    await act(async () => {
      resolveSingle({
        skills: [skill("a", "one", "older-single")],
        failures: [],
      });
      await adding;
    });
    expect(queryClient.getQueryState(DISCOVERABLE)?.fetchStatus).toBe(
      "fetching",
    );
    const newer = { skills: [skill("a", "one", "new-full")], failures: [] };
    await act(async () => {
      resolveFull(newer);
      await refreshing;
    });
    await vi.waitFor(() =>
      expect(discover.result.current.data).toEqual(newer.skills),
    );
    expect(localStorage.getItem("cc-switch-cache:skills.discoverable.v1")).toBe(
      JSON.stringify(newer),
    );
    queryClient.clear();
  });

  it("applies a repo change persisted after a full refresh has already started", async () => {
    const { queryClient, wrapper } = setup();
    const initial = { skills: [], failures: [] };
    apiMocks.discoverAvailable.mockResolvedValueOnce(initial);
    const discover = renderHook(() => useDiscoverableSkills(), { wrapper });
    await vi.waitFor(() => expect(discover.result.current.data).toEqual([]));
    let finishSave!: () => void;
    apiMocks.addRepo.mockReturnValue(
      new Promise<void>((resolve) => {
        finishSave = resolve;
      }),
    );
    let finishFull!: (value: SkillDiscoveryResult) => void;
    apiMocks.discoverAvailable.mockReturnValue(
      new Promise<SkillDiscoveryResult>((resolve) => {
        finishFull = resolve;
      }),
    );
    const changed = { skills: [skill("a", "one", "new-repo")], failures: [] };
    apiMocks.discoverRepo.mockResolvedValue(changed);
    const add = renderHook(() => useAddSkillRepo(), { wrapper });
    const refresh = renderHook(() => useRefreshDiscoverableSkills(), {
      wrapper,
    });
    let adding!: Promise<unknown>;
    let refreshing!: Promise<unknown>;
    act(() => {
      adding = add.result.current.mutateAsync(repoA);
    });
    await vi.waitFor(() => expect(apiMocks.addRepo).toHaveBeenCalled());
    act(() => {
      refreshing = refresh.result.current();
    });
    await vi.waitFor(() =>
      expect(apiMocks.discoverAvailable).toHaveBeenCalledTimes(2),
    );
    await act(async () => {
      finishSave();
      await adding;
    });
    await act(async () => {
      finishFull(initial);
      await refreshing;
    });
    expect(queryClient.getQueryData(DISCOVERABLE)).toEqual(changed);
    expect(localStorage.getItem("cc-switch-cache:skills.discoverable.v1")).toBe(
      JSON.stringify(changed),
    );
    queryClient.clear();
  });

  it("does not keep an obsolete pending scan after a newer empty result for the same repo", async () => {
    const { queryClient, wrapper } = setup();
    queryClient.setQueryData<SkillDiscoveryResult>(DISCOVERABLE, {
      skills: [],
      failures: [],
    });
    let resolveOlder!: (value: SkillDiscoveryResult) => void;
    apiMocks.discoverRepo
      .mockReturnValueOnce(
        new Promise<SkillDiscoveryResult>((resolve) => {
          resolveOlder = resolve;
        }),
      )
      .mockResolvedValue({ skills: [], failures: [] });
    const pending = renderHook(() => usePendingSkillRepos(), { wrapper });
    const add = renderHook(() => useAddSkillRepo(), { wrapper });
    let older!: Promise<unknown>;
    act(() => {
      older = add.result.current.mutateAsync(repoA);
    });
    await vi.waitFor(() =>
      expect(apiMocks.discoverRepo).toHaveBeenCalledTimes(1),
    );
    await act(async () => {
      await add.result.current.mutateAsync({ ...repoA, enabled: false });
      await add.result.current.mutateAsync(repoA);
    });
    await vi.waitFor(() => expect(pending.result.current).toEqual([]));
    await act(async () => {
      resolveOlder({ skills: [], failures: [] });
      await older;
    });
    queryClient.clear();
  });
});

describe("useRefreshDiscoverableSkills", () => {
  it("restarts a first discovery still in flight when importing another repository", async () => {
    localStorage.clear();
    apiMocks.discoverAvailable.mockReset();
    apiMocks.importFromApps.mockResolvedValue([]);
    let resolveOlder!: (value: SkillDiscoveryResult) => void;
    const next = {
      skills: [skill("vercel-labs", "agent-skills", "react-best-practices")],
      failures: [],
    };
    apiMocks.discoverAvailable
      .mockReturnValueOnce(
        new Promise<SkillDiscoveryResult>((resolve) => {
          resolveOlder = resolve;
        }),
      )
      .mockResolvedValue(next);
    const { queryClient, wrapper } = setup();
    const discover = renderHook(() => useDiscoverableSkills(), { wrapper });
    await vi.waitFor(() =>
      expect(apiMocks.discoverAvailable).toHaveBeenCalledTimes(1),
    );
    const importer = renderHook(() => useImportSkillsFromApps(), { wrapper });
    await act(async () => {
      await importer.result.current.mutateAsync([]);
    });
    await vi.waitFor(() =>
      expect(discover.result.current.data).toEqual(next.skills),
    );
    await act(async () => {
      resolveOlder({ skills: [], failures: [] });
    });
    expect(
      JSON.parse(
        localStorage.getItem("cc-switch-cache:skills.discoverable.v1")!,
      ).skills,
    ).toEqual(next.skills);
    discover.unmount();
    importer.unmount();
    queryClient.clear();
  });

  it.each(["before import", "after import"])(
    "revalidates persisted results when the inactive query is collected %s",
    async (collectedAt) => {
      localStorage.clear();
      apiMocks.discoverAvailable.mockReset();
      const previous = { skills: [skill("a", "one", "alpha")], failures: [] };
      apiMocks.discoverAvailable.mockResolvedValue(previous);
      apiMocks.importFromApps.mockResolvedValue([]);
      const { queryClient, wrapper } = setup();
      const initial = renderHook(() => useDiscoverableSkills(), { wrapper });
      await vi.waitFor(() =>
        expect(initial.result.current.data).toEqual(previous.skills),
      );
      await vi.waitFor(() =>
        expect(initial.result.current.isFetching).toBe(false),
      );
      initial.unmount();
      if (collectedAt === "before import")
        queryClient.removeQueries({ queryKey: DISCOVERABLE, exact: true });
      const importer = renderHook(() => useImportSkillsFromApps(), { wrapper });
      await act(async () => {
        await importer.result.current.mutateAsync([]);
      });
      if (collectedAt === "after import")
        queryClient.removeQueries({ queryKey: DISCOVERABLE, exact: true });
      expect(queryClient.getQueryData(DISCOVERABLE)).toBeUndefined();
      const next = {
        skills: [
          ...previous.skills,
          skill("vercel-labs", "agent-skills", "react-best-practices"),
        ],
        failures: [],
      };
      apiMocks.discoverAvailable.mockResolvedValue(next);
      const reopened = renderHook(() => useDiscoverableSkills(), { wrapper });
      await vi.waitFor(() =>
        expect(reopened.result.current.data).toEqual(next.skills),
      );
      reopened.unmount();
      importer.unmount();
      queryClient.clear();
    },
  );

  it.each([false, true])(
    "refreshes a remounted discovery only when its cache was invalidated: %s",
    async (invalidated) => {
      localStorage.clear();
      apiMocks.discoverAvailable.mockReset();
      const previous = { skills: [skill("a", "one", "alpha")], failures: [] };
      apiMocks.discoverAvailable.mockResolvedValue(previous);
      const { queryClient, wrapper } = setup();
      const initial = renderHook(() => useDiscoverableSkills(), { wrapper });
      await vi.waitFor(() =>
        expect(initial.result.current.data).toEqual(previous.skills),
      );
      await vi.waitFor(() =>
        expect(initial.result.current.isFetching).toBe(false),
      );
      initial.unmount();
      apiMocks.discoverAvailable.mockClear();
      const next = {
        skills: [
          ...previous.skills,
          skill("vercel-labs", "agent-skills", "react-best-practices"),
        ],
        failures: [],
      };
      apiMocks.discoverAvailable.mockResolvedValue(next);
      if (invalidated)
        await queryClient.invalidateQueries({ queryKey: DISCOVERABLE });
      const reopened = renderHook(() => useDiscoverableSkills(), { wrapper });
      if (invalidated) {
        await vi.waitFor(() =>
          expect(reopened.result.current.data).toEqual(next.skills),
        );
        expect(apiMocks.discoverAvailable).toHaveBeenCalledTimes(1);
      } else {
        expect(reopened.result.current.data).toEqual(previous.skills);
        expect(apiMocks.discoverAvailable).not.toHaveBeenCalled();
      }
      reopened.unmount();
      queryClient.clear();
    },
  );

  it.each(["failure", "success"])(
    "does not persist a cancelled older %s after a newer successful empty scan",
    async (lateResult) => {
      localStorage.clear();
      apiMocks.discoverAvailable.mockReset();
      const previous = { skills: [skill("a", "one", "alpha")], failures: [] };
      apiMocks.discoverAvailable.mockResolvedValue(previous);
      const { queryClient, wrapper } = setup();
      const discover = renderHook(() => useDiscoverableSkills(), { wrapper });
      const refresh = renderHook(() => useRefreshDiscoverableSkills(), {
        wrapper,
      });
      await vi.waitFor(() =>
        expect(discover.result.current.isFetching).toBe(false),
      );
      await act(async () => {
        await refresh.result.current();
      });
      let resolveOlder!: (value: SkillDiscoveryResult) => void;
      apiMocks.discoverAvailable.mockReset();
      apiMocks.discoverAvailable
        .mockReturnValueOnce(
          new Promise<SkillDiscoveryResult>((resolve) => {
            resolveOlder = resolve;
          }),
        )
        .mockResolvedValue({ skills: [], failures: [] });
      act(() => {
        void refresh.result.current();
      });
      await vi.waitFor(() =>
        expect(apiMocks.discoverAvailable).toHaveBeenCalledTimes(1),
      );
      await act(async () => {
        await queryClient.invalidateQueries({ queryKey: DISCOVERABLE });
      });
      expect(
        queryClient.getQueryData<SkillDiscoveryResult>(DISCOVERABLE)?.skills,
      ).toEqual([]);
      await act(async () => {
        resolveOlder(
          lateResult === "success"
            ? previous
            : {
                skills: [],
                failures: [
                  { owner: "a", name: "one", branch: "main", error: "timeout" },
                ],
              },
        );
      });
      expect(
        JSON.parse(
          localStorage.getItem("cc-switch-cache:skills.discoverable.v1")!,
        ).skills,
      ).toEqual([]);
      expect(
        queryClient.getQueryData<SkillDiscoveryResult>(DISCOVERABLE)?.skills,
      ).toEqual([]);
      queryClient.clear();
    },
  );

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
