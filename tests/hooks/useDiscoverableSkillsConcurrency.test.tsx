import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useDiscoverableSkills, useRetrySkillRepo } from "@/hooks/useSkills";
import {
  skillsApi,
  type DiscoverableSkill,
  type SkillDiscoveryResult,
  type SkillRepo,
} from "@/lib/api/skills";
import {
  getSkillDiscoveryTaskSnapshot,
  resetSkillDiscoveryTask,
} from "@/stores/skillDiscoveryTask";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function makeSkill(
  name: string,
  owner = "owner",
  repoName = "repo",
): DiscoverableSkill {
  return {
    key: `${owner}/${repoName}:${name.toLowerCase()}`,
    name,
    description: "",
    directory: name.toLowerCase(),
    repoOwner: owner,
    repoName,
    repoBranch: "main",
  };
}

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
}

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("useDiscoverableSkills concurrency", () => {
  beforeEach(() => {
    resetSkillDiscoveryTask();
    vi.restoreAllMocks();
    vi.spyOn(skillsApi, "getCachedDiscoverable").mockResolvedValue({
      skills: [],
      failures: [],
    });
  });

  it("shows persisted skills while the network refresh is still pending", async () => {
    const fullRefresh = deferred<SkillDiscoveryResult>();
    vi.mocked(skillsApi.getCachedDiscoverable).mockResolvedValue({
      skills: [makeSkill("Cached")],
      failures: [],
    });
    vi.spyOn(skillsApi, "discoverAvailable").mockReturnValue(
      fullRefresh.promise,
    );
    const queryClient = createQueryClient();
    queryClient.setQueryData(
      ["skills", "repos"],
      [
        {
          owner: "owner",
          name: "repo",
          branch: "main",
          enabled: true,
        },
      ],
    );

    const { result } = renderHook(() => useDiscoverableSkills(), {
      wrapper: createWrapper(queryClient),
    });

    await waitFor(() =>
      expect(result.current.data?.skills[0]?.name).toBe("Cached"),
    );
    expect(result.current.isFetching).toBe(true);

    await act(async () => {
      fullRefresh.resolve({
        skills: [makeSkill("Fresh")],
        failures: [],
      });
      await fullRefresh.promise;
    });
  });

  it("does not let an older full refresh overwrite a newer repository retry", async () => {
    const fullRefresh = deferred<SkillDiscoveryResult>();
    const repo: SkillRepo = {
      owner: "owner",
      name: "repo",
      branch: "main",
      enabled: true,
    };
    vi.spyOn(skillsApi, "discoverAvailable").mockReturnValue(
      fullRefresh.promise,
    );
    vi.spyOn(skillsApi, "discoverRepo").mockResolvedValue({
      skills: [makeSkill("Targeted")],
      failures: [],
    });
    const queryClient = createQueryClient();
    queryClient.setQueryData(["skills", "repos"], [repo]);

    const { result } = renderHook(
      () => ({
        discovery: useDiscoverableSkills(),
        retry: useRetrySkillRepo(),
      }),
      { wrapper: createWrapper(queryClient) },
    );

    await waitFor(() =>
      expect(skillsApi.discoverAvailable).toHaveBeenCalledTimes(1),
    );
    await act(async () => {
      await result.current.retry.mutateAsync(repo);
    });
    expect(
      queryClient.getQueryData<SkillDiscoveryResult>(["skills", "discoverable"])
        ?.skills[0]?.name,
    ).toBe("Targeted");

    await act(async () => {
      fullRefresh.resolve({
        skills: [makeSkill("Stale Full")],
        failures: [],
      });
      await fullRefresh.promise;
    });

    await waitFor(() =>
      expect(
        queryClient.getQueryData<SkillDiscoveryResult>([
          "skills",
          "discoverable",
        ])?.skills[0]?.name,
      ).toBe("Targeted"),
    );
    expect(getSkillDiscoveryTaskSnapshot().active).toBe(false);
  });

  it("keeps unrelated results from a full refresh when one repository was retried", async () => {
    const fullRefresh = deferred<SkillDiscoveryResult>();
    const retriedRepo: SkillRepo = {
      owner: "first",
      name: "skills",
      branch: "main",
      enabled: true,
    };
    const otherRepo: SkillRepo = {
      owner: "second",
      name: "skills",
      branch: "main",
      enabled: true,
    };
    vi.spyOn(skillsApi, "discoverAvailable").mockReturnValue(
      fullRefresh.promise,
    );
    vi.spyOn(skillsApi, "discoverRepo").mockResolvedValue({
      skills: [makeSkill("Targeted First", "first", "skills")],
      failures: [],
    });
    const queryClient = createQueryClient();
    queryClient.setQueryData(["skills", "repos"], [retriedRepo, otherRepo]);

    const { result } = renderHook(
      () => ({
        discovery: useDiscoverableSkills(),
        retry: useRetrySkillRepo(),
      }),
      { wrapper: createWrapper(queryClient) },
    );

    await waitFor(() =>
      expect(skillsApi.discoverAvailable).toHaveBeenCalledTimes(1),
    );
    await act(async () => {
      await result.current.retry.mutateAsync(retriedRepo);
    });

    await act(async () => {
      fullRefresh.resolve({
        skills: [
          makeSkill("Stale First", "first", "skills"),
          makeSkill("Fresh Second", "second", "skills"),
        ],
        failures: [],
      });
      await fullRefresh.promise;
    });

    await waitFor(() =>
      expect(
        queryClient
          .getQueryData<SkillDiscoveryResult>(["skills", "discoverable"])
          ?.skills.map((skill) => skill.name)
          .sort(),
      ).toEqual(["Fresh Second", "Targeted First"]),
    );
  });

  it("merges repository retries into persisted discovery cache before the full refresh completes", async () => {
    const fullRefresh = deferred<SkillDiscoveryResult>();
    const retriedRepo: SkillRepo = {
      owner: "first",
      name: "skills",
      branch: "main",
      enabled: true,
    };
    const otherRepo: SkillRepo = {
      owner: "second",
      name: "skills",
      branch: "main",
      enabled: true,
    };
    vi.mocked(skillsApi.getCachedDiscoverable).mockResolvedValue({
      skills: [
        makeSkill("Cached First", "first", "skills"),
        makeSkill("Cached Second", "second", "skills"),
      ],
      failures: [],
    });
    vi.spyOn(skillsApi, "discoverAvailable").mockReturnValue(
      fullRefresh.promise,
    );
    vi.spyOn(skillsApi, "discoverRepo").mockResolvedValue({
      skills: [makeSkill("Fresh First", "first", "skills")],
      failures: [],
    });
    const queryClient = createQueryClient();
    queryClient.setQueryData(["skills", "repos"], [retriedRepo, otherRepo]);

    const { result } = renderHook(
      () => ({
        discovery: useDiscoverableSkills(),
        retry: useRetrySkillRepo(),
      }),
      { wrapper: createWrapper(queryClient) },
    );

    await waitFor(() =>
      expect(
        result.current.discovery.data?.skills.map((skill) => skill.name),
      ).toEqual(["Cached First", "Cached Second"]),
    );

    await act(async () => {
      await result.current.retry.mutateAsync(retriedRepo);
    });

    expect(
      queryClient
        .getQueryData<SkillDiscoveryResult>(["skills", "discoverable"])
        ?.skills.map((skill) => skill.name)
        .sort(),
    ).toEqual(["Cached Second", "Fresh First"]);
  });
});
