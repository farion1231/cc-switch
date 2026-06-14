import type { PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  useRemoveSkillRepo,
  useRestoreSkillBackup,
  useUninstallSkill,
} from "@/hooks/useSkills";
import {
  skillsApi,
  type InstalledSkill,
  type SkillRepo,
} from "@/lib/api/skills";
import {
  applySkillDiscoveryProgress,
  beginSkillDiscovery,
  getSkillDiscoveryTaskSnapshot,
  resetSkillDiscoveryTask,
} from "@/stores/skillDiscoveryTask";

describe("useRemoveSkillRepo", () => {
  beforeEach(() => {
    resetSkillDiscoveryTask();
    vi.restoreAllMocks();
  });

  it("removes the repository from caches and active progress immediately", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    const repos: SkillRepo[] = [
      {
        owner: "anthropics",
        name: "skills",
        branch: "main",
        enabled: true,
      },
      {
        owner: "removed",
        name: "repo",
        branch: "main",
        enabled: true,
      },
    ];
    queryClient.setQueryData(["skills", "repos"], repos);
    queryClient.setQueryData(["skills", "discoverable"], {
      skills: [
        {
          key: "removed/repo:stale",
          name: "Stale",
          description: "",
          directory: "stale",
          repoOwner: "removed",
          repoName: "repo",
          repoBranch: "main",
        },
      ],
      failures: [],
      refreshedRepositories: [
        {
          owner: "anthropics",
          name: "skills",
          branch: "main",
        },
        {
          owner: "removed",
          name: "repo",
          branch: "main",
        },
      ],
    });

    beginSkillDiscovery();
    applySkillDiscoveryProgress({
      phase: "scanning",
      completed: 0,
      total: 2,
      repo: "removed/repo",
    });
    vi.spyOn(skillsApi, "removeRepo").mockResolvedValue(true);

    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useRemoveSkillRepo(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ owner: "removed", name: "repo" });
    });

    expect(queryClient.getQueryData<SkillRepo[]>(["skills", "repos"])).toEqual([
      repos[0],
    ]);
    expect(
      queryClient.getQueryData<{
        skills: unknown[];
        refreshedRepositories?: SkillRepo[];
      }>(["skills", "discoverable"]),
    ).toEqual(
      expect.objectContaining({
        skills: [],
        refreshedRepositories: [
          {
            owner: "anthropics",
            name: "skills",
            branch: "main",
          },
        ],
      }),
    );
    expect(
      getSkillDiscoveryTaskSnapshot().repositories["removed/repo"],
    ).toBeUndefined();
  });

  it("optimistically removes the repository from caches before backend cleanup finishes", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    const repos: SkillRepo[] = [
      {
        owner: "anthropics",
        name: "skills",
        branch: "main",
        enabled: true,
      },
      {
        owner: "removed",
        name: "repo",
        branch: "main",
        enabled: true,
      },
    ];
    queryClient.setQueryData(["skills", "repos"], repos);
    queryClient.setQueryData(["skills", "discoverable"], {
      skills: [
        {
          key: "removed/repo:stale",
          name: "Stale",
          description: "",
          directory: "stale",
          repoOwner: "removed",
          repoName: "repo",
          repoBranch: "main",
        },
      ],
      failures: [
        {
          owner: "removed",
          name: "repo",
          branch: "main",
          error: "network",
        },
      ],
      refreshedRepositories: [
        {
          owner: "removed",
          name: "repo",
          branch: "main",
        },
      ],
    });
    let resolveRemove!: (value: boolean) => void;
    vi.spyOn(skillsApi, "removeRepo").mockReturnValue(
      new Promise<boolean>((resolve) => {
        resolveRemove = resolve;
      }),
    );

    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useRemoveSkillRepo(), { wrapper });

    act(() => {
      result.current.mutate({ owner: "removed", name: "repo" });
    });

    await waitFor(() =>
      expect(
        queryClient.getQueryData<SkillRepo[]>(["skills", "repos"]),
      ).toEqual([repos[0]]),
    );
    expect(
      queryClient.getQueryData<{ skills: unknown[]; failures: unknown[] }>([
        "skills",
        "discoverable",
      ]),
    ).toEqual(expect.objectContaining({ skills: [], failures: [] }));

    await act(async () => {
      resolveRemove(true);
    });
  });

  it("removes stale update-check entries when a skill is uninstalled", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    queryClient.setQueryData(
      ["skills", "installed"],
      [
        {
          id: "anthropics/skills:frontend-design",
          name: "Frontend Design",
          directory: "frontend-design",
          repoOwner: "anthropics",
          repoName: "skills",
          repoBranch: "main",
          apps: {},
          installedAt: 0,
          updatedAt: 0,
        },
      ],
    );
    queryClient.setQueryData(["skills", "updates"], {
      updates: [
        {
          id: "anthropics/skills:frontend-design",
          name: "Frontend Design",
          remoteHash: "remote",
          status: "updateAvailable",
        },
      ],
      failures: [],
    });
    vi.spyOn(skillsApi, "uninstallUnified").mockResolvedValue({});

    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useUninstallSkill(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({
        id: "anthropics/skills:frontend-design",
        skillKey: "frontend-design:anthropics:skills",
      });
    });

    expect(
      queryClient.getQueryData<{ updates: unknown[] }>(["skills", "updates"])
        ?.updates,
    ).toEqual([]);
  });

  it("removes stale update-check failures when the last skill from a repository is uninstalled", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    queryClient.setQueryData(
      ["skills", "installed"],
      [
        {
          id: "anthropics/skills:frontend-design",
          name: "Frontend Design",
          directory: "frontend-design",
          repoOwner: "anthropics",
          repoName: "skills",
          repoBranch: "main",
          apps: {},
          installedAt: 0,
          updatedAt: 0,
        },
      ],
    );
    queryClient.setQueryData(["skills", "updates"], {
      updates: [],
      failures: [
        {
          owner: "anthropics",
          name: "skills",
          branch: "main",
          error: "network",
        },
      ],
    });
    vi.spyOn(skillsApi, "uninstallUnified").mockResolvedValue({});

    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useUninstallSkill(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({
        id: "anthropics/skills:frontend-design",
        skillKey: "frontend-design:anthropics:skills",
      });
    });

    expect(
      queryClient.getQueryData<{ failures: unknown[] }>(["skills", "updates"])
        ?.failures,
    ).toEqual([]);
  });

  it("removes the previous update result when a backup changes the skill contents", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    const restoredSkill: InstalledSkill = {
      id: "anthropics/skills:frontend-design",
      name: "Frontend Design",
      directory: "frontend-design",
      repoOwner: "anthropics",
      repoName: "skills",
      repoBranch: "main",
      apps: {
        claude: true,
        codex: false,
        gemini: false,
        opencode: false,
        openclaw: false,
        hermes: false,
      },
      installedAt: 0,
      updatedAt: 0,
      contentHash: "restored",
    };
    queryClient.setQueryData(
      ["skills", "installed"],
      [{ ...restoredSkill, contentHash: "before-restore" }],
    );
    queryClient.setQueryData(["skills", "updates"], {
      updates: [
        {
          id: restoredSkill.id,
          name: restoredSkill.name,
          remoteHash: "remote",
          status: "updateAvailable",
        },
      ],
      failures: [],
    });
    vi.spyOn(skillsApi, "restoreBackup").mockResolvedValue(restoredSkill);

    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useRestoreSkillBackup(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({
        backupId: "backup",
        currentApp: "claude",
      });
    });

    expect(
      queryClient.getQueryData<{ updates: unknown[] }>(["skills", "updates"])
        ?.updates,
    ).toEqual([]);
  });
});
