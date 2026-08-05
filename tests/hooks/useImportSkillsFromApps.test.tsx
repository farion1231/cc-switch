import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { mergeImportedSkills } from "@/hooks/useSkills.helpers";
import {
  useImportSkillsFromApps,
  useInstallSkill,
  useInstallSkillsFromZip,
  useUninstallSkill,
} from "@/hooks/useSkills";
import type { DiscoverableSkill, InstalledSkill } from "@/lib/api/skills";

const apiMocks = vi.hoisted(() => ({
  importFromApps: vi.fn(),
  installFromZip: vi.fn(),
  installUnified: vi.fn(),
  uninstallUnified: vi.fn(),
}));

vi.mock("@/lib/api/skills", () => ({
  skillsApi: apiMocks,
}));

function makeSkill(overrides: Partial<InstalledSkill> = {}): InstalledSkill {
  return {
    id: "skill-a",
    name: "Skill A",
    directory: "skill-a",
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
    ...overrides,
  };
}

function makeDiscoverableSkill(): DiscoverableSkill {
  return {
    key: "owner/repo:skill-a",
    name: "Skill A",
    description: "Skill A description",
    directory: "skill-a",
    repoOwner: "owner",
    repoName: "repo",
    repoBranch: "main",
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

// Regression coverage for issue #2139: when a user double-clicks the import
// button (or the mutation otherwise fires twice with the same payload), the
// installed cache must not accumulate duplicate entries for the same skill.
describe("mergeImportedSkills", () => {
  it("returns the imported list as-is when no cache exists yet", () => {
    const imported = [makeSkill()];
    expect(mergeImportedSkills(undefined, imported)).toEqual(imported);
  });

  it("dedupes by id when the same skill is imported twice in a row", () => {
    const existing = [makeSkill()];
    const secondImport = [makeSkill()];
    const merged = mergeImportedSkills(existing, secondImport);
    expect(merged).toHaveLength(1);
    expect(merged[0]).toBe(secondImport[0]);
  });

  it("replaces stale cache entries with fresh imports for the same id", () => {
    const stale = [makeSkill({ name: "Stale Name" })];
    const fresh = [makeSkill({ name: "Fresh Name" })];
    const merged = mergeImportedSkills(stale, fresh);
    expect(merged).toHaveLength(1);
    expect(merged[0].name).toBe("Fresh Name");
  });

  it("returns the existing reference unchanged when the imported list is empty", () => {
    const existing = [makeSkill()];
    expect(mergeImportedSkills(existing, [])).toBe(existing);
  });

  it("appends newly imported skills without dropping existing unrelated ones", () => {
    const existing = [makeSkill({ id: "skill-a", directory: "skill-a" })];
    const imported = [
      makeSkill({ id: "skill-b", directory: "skill-b", name: "Skill B" }),
    ];
    const merged = mergeImportedSkills(existing, imported);
    expect(merged.map((s) => s.id).sort()).toEqual(["skill-a", "skill-b"]);
  });

  it("dedupes repeated IDs within the incoming list and keeps the last value", () => {
    const first = makeSkill({ name: "First Value" });
    const last = makeSkill({ name: "Last Value" });

    const merged = mergeImportedSkills(undefined, [first, last]);

    expect(merged).toEqual([last]);
  });

  it("also removes duplicate IDs already present in stale cache data", () => {
    const stale = makeSkill({ name: "Stale Value" });
    const newer = makeSkill({ name: "Newer Value" });
    const imported = makeSkill({ id: "skill-b", name: "Skill B" });

    const merged = mergeImportedSkills([stale, newer], [imported]);

    expect(merged).toEqual([newer, imported]);
  });
});

describe("Skills install and import mutation hooks", () => {
  beforeEach(() => {
    apiMocks.importFromApps.mockReset();
    apiMocks.installFromZip.mockReset();
    apiMocks.installUnified.mockReset();
    apiMocks.uninstallUnified.mockReset();
  });

  it("merges a successful install by ID without mutating discoverable cache", async () => {
    const queryClient = createQueryClient();
    const stale = makeSkill({ name: "Stale Skill" });
    const unrelated = makeSkill({ id: "skill-b", name: "Skill B" });
    const installed = makeSkill({ name: "Fresh Skill" });
    const discoverable = [makeDiscoverableSkill()];
    queryClient.setQueryData(["skills", "installed"], [stale, unrelated]);
    queryClient.setQueryData(["skills", "discoverable"], discoverable);
    apiMocks.installUnified.mockResolvedValueOnce(installed);
    const { result } = renderHook(() => useInstallSkill(), {
      wrapper: createWrapper(queryClient),
    });

    await act(async () => {
      await result.current.mutateAsync({
        skill: makeDiscoverableSkill(),
        currentApp: "claude",
      });
    });

    expect(
      queryClient.getQueryData<InstalledSkill[]>(["skills", "installed"]),
    ).toEqual([installed, unrelated]);
    expect(queryClient.getQueryData(["skills", "discoverable"])).toBe(
      discoverable,
    );
  });

  it("merges ZIP results without duplicate IDs", async () => {
    const queryClient = createQueryClient();
    const stale = makeSkill({ name: "Stale Skill" });
    const first = makeSkill({ name: "First ZIP Value" });
    const last = makeSkill({ name: "Last ZIP Value" });
    const second = makeSkill({ id: "skill-b", name: "Skill B" });
    queryClient.setQueryData(["skills", "installed"], [stale]);
    apiMocks.installFromZip.mockResolvedValueOnce([first, last, second]);
    const { result } = renderHook(() => useInstallSkillsFromZip(), {
      wrapper: createWrapper(queryClient),
    });

    await act(async () => {
      await result.current.mutateAsync({
        filePath: "C:\\skills.zip",
        currentApp: "claude",
      });
    });

    expect(
      queryClient.getQueryData<InstalledSkill[]>(["skills", "installed"]),
    ).toEqual([last, second]);
  });

  it("merges imported results and refreshes every affected collection", async () => {
    const queryClient = createQueryClient();
    const stale = makeSkill({ name: "Stale Skill" });
    const imported = makeSkill({ name: "Imported Skill" });
    queryClient.setQueryData(["skills", "installed"], [stale]);
    apiMocks.importFromApps.mockResolvedValueOnce([imported, imported]);
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useImportSkillsFromApps(), {
      wrapper: createWrapper(queryClient),
    });

    await act(async () => {
      await result.current.mutateAsync([]);
    });

    expect(
      queryClient.getQueryData<InstalledSkill[]>(["skills", "installed"]),
    ).toEqual([imported]);
    for (const queryKey of [
      ["skills", "installed"],
      ["skills", "unmanaged"],
      ["skills", "repos"],
      ["skills", "discoverable"],
    ]) {
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey });
    }
  });

  it("keeps a rejected install pending until installed and unmanaged refresh", async () => {
    let releaseInvalidation: (() => void) | undefined;
    const invalidationPending = new Promise<void>((resolve) => {
      releaseInvalidation = resolve;
    });
    const queryClient = createQueryClient();
    apiMocks.installUnified.mockRejectedValueOnce(new Error("sync failed"));
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockImplementation(() => invalidationPending);
    const { result } = renderHook(() => useInstallSkill(), {
      wrapper: createWrapper(queryClient),
    });

    let mutation!: Promise<InstalledSkill>;
    act(() => {
      mutation = result.current.mutateAsync({
        skill: makeDiscoverableSkill(),
        currentApp: "claude",
      });
      void mutation.catch(() => undefined);
    });

    await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(2));
    expect(result.current.isPending).toBe(true);
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "installed"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "unmanaged"],
    });

    releaseInvalidation?.();
    await act(async () => {
      await expect(mutation).rejects.toThrow("sync failed");
    });
    await waitFor(() => expect(result.current.isPending).toBe(false));
  });

  it("refreshes installed and unmanaged when ZIP install rejects", async () => {
    const queryClient = createQueryClient();
    apiMocks.installFromZip.mockRejectedValueOnce(new Error("sync failed"));
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useInstallSkillsFromZip(), {
      wrapper: createWrapper(queryClient),
    });

    await act(async () => {
      await expect(
        result.current.mutateAsync({
          filePath: "C:\\skills.zip",
          currentApp: "claude",
        }),
      ).rejects.toThrow("sync failed");
    });

    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "installed"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "unmanaged"],
    });
  });

  it("keeps a rejected import pending until all affected caches refresh", async () => {
    let releaseInvalidation: (() => void) | undefined;
    const invalidationPending = new Promise<void>((resolve) => {
      releaseInvalidation = resolve;
    });
    const queryClient = createQueryClient();
    apiMocks.importFromApps.mockRejectedValueOnce(new Error("import failed"));
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockImplementation(() => invalidationPending);
    const { result } = renderHook(() => useImportSkillsFromApps(), {
      wrapper: createWrapper(queryClient),
    });

    let mutation!: Promise<InstalledSkill[]>;
    act(() => {
      mutation = result.current.mutateAsync([]);
      void mutation.catch(() => undefined);
    });

    await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(4));
    expect(result.current.isPending).toBe(true);
    for (const queryKey of [
      ["skills", "installed"],
      ["skills", "unmanaged"],
      ["skills", "repos"],
      ["skills", "discoverable"],
    ]) {
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey });
    }

    releaseInvalidation?.();
    await act(async () => {
      await expect(mutation).rejects.toThrow("import failed");
    });
    await waitFor(() => expect(result.current.isPending).toBe(false));
  });

  it("does not rewrite discoverable cache when uninstall succeeds", async () => {
    const queryClient = createQueryClient();
    const discoverable = [makeDiscoverableSkill()];
    queryClient.setQueryData(["skills", "installed"], [makeSkill()]);
    queryClient.setQueryData(["skills", "discoverable"], discoverable);
    apiMocks.uninstallUnified.mockResolvedValueOnce({ backupPath: undefined });
    const { result } = renderHook(() => useUninstallSkill(), {
      wrapper: createWrapper(queryClient),
    });

    await act(async () => {
      await result.current.mutateAsync("skill-a");
    });

    expect(
      queryClient.getQueryData<InstalledSkill[]>(["skills", "installed"]),
    ).toEqual([]);
    expect(queryClient.getQueryData(["skills", "discoverable"])).toBe(
      discoverable,
    );
  });
});
