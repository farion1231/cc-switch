import type { PropsWithChildren } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useResyncSkillsToApps, useUpdateSkill } from "@/hooks/useSkills";
import type {
  InstalledSkill,
  SkillUpdateCheckResult,
  SkillUpdateOutcome,
} from "@/lib/api/skills";

const apiMocks = vi.hoisted(() => ({
  updateSkill: vi.fn(),
  resyncToApps: vi.fn(),
}));

vi.mock("@/lib/api/skills", () => ({
  skillsApi: apiMocks,
}));

function makeSkill(overrides: Partial<InstalledSkill> = {}): InstalledSkill {
  return {
    id: "owner/repo:alpha-skill",
    name: "Alpha Skill",
    directory: "alpha-skill",
    apps: {
      claude: true,
      codex: true,
      gemini: false,
      opencode: false,
      openclaw: false,
      hermes: false,
      pi: false,
    },
    installedAt: 1,
    updatedAt: 1,
    contentHash: "hash-v1",
    ...overrides,
  };
}

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      mutations: { retry: false },
      queries: { retry: false },
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

describe("useUpdateSkill and useResyncSkillsToApps", () => {
  beforeEach(() => {
    apiMocks.updateSkill.mockReset();
    apiMocks.resyncToApps.mockReset();
  });

  it("keeps the update entry when client sync fails, strips outcome fields from installed cache, and clears update after resync converges", async () => {
    const queryClient = createQueryClient();
    const initial = makeSkill();
    queryClient.setQueryData<InstalledSkill[]>(
      ["skills", "installed"],
      [initial],
    );
    queryClient.setQueryData<SkillUpdateCheckResult>(["skills", "updates"], {
      updates: [
        {
          id: initial.id,
          name: initial.name,
          currentHash: "hash-v1",
          remoteHash: "hash-v2",
        },
        {
          id: "owner/repo:other-skill",
          name: "Other Skill",
          currentHash: "other-v1",
          remoteHash: "other-v2",
        },
      ],
      failures: [],
    });

    const partialOutcome: SkillUpdateOutcome = {
      ...initial,
      name: "Alpha Skill v2",
      contentHash: "hash-v2",
      updatedAt: 2,
      failedApps: ["codex"],
      syncWarnings: ["codex: Permission denied (~/.codex/skills)"],
    };
    apiMocks.updateSkill.mockResolvedValueOnce(partialOutcome);

    const wrapper = createWrapper(queryClient);
    const { result: updateHook } = renderHook(() => useUpdateSkill(), {
      wrapper,
    });

    await act(async () => {
      await updateHook.current.mutateAsync(initial.id);
    });

    const installedAfterPartial = queryClient.getQueryData<InstalledSkill[]>([
      "skills",
      "installed",
    ]);
    expect(installedAfterPartial).toHaveLength(1);
    expect(installedAfterPartial?.[0]?.name).toBe("Alpha Skill v2");
    expect(installedAfterPartial?.[0]?.contentHash).toBe("hash-v2");
    expect("failedApps" in (installedAfterPartial?.[0] ?? {})).toBe(false);
    expect("syncWarnings" in (installedAfterPartial?.[0] ?? {})).toBe(false);

    // Update entry must stay in ["skills", "updates"] while client sync is incomplete
    const updatesAfterPartial =
      queryClient.getQueryData<SkillUpdateCheckResult>(["skills", "updates"]);
    expect(updatesAfterPartial?.updates.map((u) => u.id)).toEqual([
      initial.id,
      "owner/repo:other-skill",
    ]);

    // If resync still reports a failure, keep the update entry
    apiMocks.resyncToApps.mockResolvedValueOnce([
      { app: "claude", ok: true, failedSkills: [] },
      {
        app: "codex",
        ok: false,
        failedSkills: [
          { directory: "alpha-skill", error: "Permission denied" },
        ],
      },
    ]);
    const { result: resyncHook } = renderHook(() => useResyncSkillsToApps(), {
      wrapper,
    });
    await act(async () => {
      await resyncHook.current.mutateAsync();
    });
    expect(
      queryClient
        .getQueryData<SkillUpdateCheckResult>(["skills", "updates"])
        ?.updates.map((u) => u.id),
    ).toEqual([initial.id, "owner/repo:other-skill"]);

    // Once resync succeeds for all apps, only converged skills (contentHash === remoteHash) leave updates
    apiMocks.resyncToApps.mockResolvedValueOnce([
      { app: "claude", ok: true, failedSkills: [] },
      { app: "codex", ok: true, failedSkills: [] },
    ]);
    await act(async () => {
      await resyncHook.current.mutateAsync();
    });
    expect(
      queryClient
        .getQueryData<SkillUpdateCheckResult>(["skills", "updates"])
        ?.updates.map((u) => u.id),
    ).toEqual(["owner/repo:other-skill"]);
  });

  it("removes the update entry immediately when all enabled client syncs succeed", async () => {
    const queryClient = createQueryClient();
    const initial = makeSkill();
    queryClient.setQueryData<InstalledSkill[]>(
      ["skills", "installed"],
      [initial],
    );
    queryClient.setQueryData<SkillUpdateCheckResult>(["skills", "updates"], {
      updates: [
        {
          id: initial.id,
          name: initial.name,
          currentHash: "hash-v1",
          remoteHash: "hash-v2",
        },
      ],
      failures: [],
    });

    apiMocks.updateSkill.mockResolvedValueOnce({
      ...initial,
      contentHash: "hash-v2",
      updatedAt: 2,
    });

    const { result } = renderHook(() => useUpdateSkill(), {
      wrapper: createWrapper(queryClient),
    });

    await act(async () => {
      await result.current.mutateAsync(initial.id);
    });

    expect(
      queryClient.getQueryData<SkillUpdateCheckResult>(["skills", "updates"])
        ?.updates,
    ).toEqual([]);
  });
});
