import type { PropsWithChildren } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  useBulkToggleSkillApp,
  useDeleteSkillBackup,
  useRestoreSkillBackup,
  useToggleSkillApp,
  useUninstallSkill,
  useUpdateSkill,
} from "@/hooks/useSkills";
import type { SkillBackupEntry, SkillUpdateInfo } from "@/lib/api/skills";

const toggleAppMock = vi.hoisted(() => vi.fn());
const restoreBackupMock = vi.hoisted(() => vi.fn());
const uninstallMock = vi.hoisted(() => vi.fn());
const deleteBackupMock = vi.hoisted(() => vi.fn());
const updateSkillMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api/skills", () => ({
  skillsApi: {
    toggleApp: toggleAppMock,
    restoreBackup: restoreBackupMock,
    uninstallUnified: uninstallMock,
    deleteBackup: deleteBackupMock,
    updateSkill: updateSkillMock,
  },
}));

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("Skills management mutation hooks", () => {
  beforeEach(() => {
    toggleAppMock.mockReset();
    restoreBackupMock.mockReset();
    uninstallMock.mockReset();
    deleteBackupMock.mockReset();
    updateSkillMock.mockReset();
  });

  it("stays pending until the refreshed skill list is available", async () => {
    let releaseInvalidation: (() => void) | undefined;
    const invalidationPending = new Promise<void>((resolve) => {
      releaseInvalidation = resolve;
    });
    toggleAppMock.mockResolvedValue(undefined);
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockImplementation(() => invalidationPending);
    const { result } = renderHook(() => useBulkToggleSkillApp(), {
      wrapper: createWrapper(queryClient),
    });

    let mutation!: Promise<unknown>;
    act(() => {
      mutation = result.current.mutateAsync({
        ids: ["alpha", "beta"],
        app: "claude",
        enabled: true,
      });
    });

    await waitFor(() => expect(toggleAppMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(1));
    expect(result.current.isPending).toBe(true);

    releaseInvalidation?.();
    await act(async () => {
      await mutation;
    });

    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "installed"],
    });
    await waitFor(() => expect(result.current.isPending).toBe(false));
  });

  it("keeps a single toggle pending until the refreshed list is available", async () => {
    let releaseInvalidation: (() => void) | undefined;
    const invalidationPending = new Promise<void>((resolve) => {
      releaseInvalidation = resolve;
    });
    toggleAppMock.mockResolvedValueOnce(undefined);
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockImplementation(() => invalidationPending);
    const { result } = renderHook(() => useToggleSkillApp(), {
      wrapper: createWrapper(queryClient),
    });

    let mutation!: Promise<unknown>;
    act(() => {
      mutation = result.current.mutateAsync({
        id: "alpha",
        app: "claude",
        enabled: true,
      });
    });

    await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(1));
    expect(result.current.isPending).toBe(true);

    releaseInvalidation?.();
    await act(async () => {
      await mutation;
    });
    await waitFor(() => expect(result.current.isPending).toBe(false));
  });

  it("keeps backup restore pending until installed skills and backups refresh", async () => {
    let releaseInvalidation: (() => void) | undefined;
    const invalidationPending = new Promise<void>((resolve) => {
      releaseInvalidation = resolve;
    });
    restoreBackupMock.mockResolvedValueOnce(undefined);
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockImplementation(() => invalidationPending);
    const { result } = renderHook(() => useRestoreSkillBackup(), {
      wrapper: createWrapper(queryClient),
    });

    let mutation!: Promise<unknown>;
    act(() => {
      mutation = result.current.mutateAsync({
        backupId: "backup-1",
        currentApp: "claude",
      });
    });

    await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(2));
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "installed"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "backups"],
    });
    expect(result.current.isPending).toBe(true);

    releaseInvalidation?.();
    await act(async () => {
      await mutation;
    });
    await waitFor(() => expect(result.current.isPending).toBe(false));
  });

  it("also refreshes installed skills and backups when restore rejects", async () => {
    let releaseInvalidation: (() => void) | undefined;
    const invalidationPending = new Promise<void>((resolve) => {
      releaseInvalidation = resolve;
    });
    restoreBackupMock.mockRejectedValueOnce(new Error("sync failed"));
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockImplementation(() => invalidationPending);
    const { result } = renderHook(() => useRestoreSkillBackup(), {
      wrapper: createWrapper(queryClient),
    });

    let mutation!: Promise<unknown>;
    act(() => {
      mutation = result.current.mutateAsync({
        backupId: "backup-1",
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
      queryKey: ["skills", "backups"],
    });

    releaseInvalidation?.();
    await act(async () => {
      await expect(mutation).rejects.toThrow("sync failed");
    });
    await waitFor(() => expect(result.current.isPending).toBe(false));
  });

  it("removes an uninstalled Skill from cached update results", async () => {
    uninstallMock.mockResolvedValueOnce({ backupPath: null });
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    queryClient.setQueryData<SkillUpdateInfo[]>(
      ["skills", "updates"],
      [
        { id: "alpha", name: "Alpha", remoteHash: "alpha-remote" },
        { id: "beta", name: "Beta", remoteHash: "beta-remote" },
      ],
    );
    const { result } = renderHook(() => useUninstallSkill(), {
      wrapper: createWrapper(queryClient),
    });

    await act(async () => {
      await result.current.mutateAsync("alpha");
    });

    expect(
      queryClient.getQueryData<SkillUpdateInfo[]>(["skills", "updates"]),
    ).toEqual([{ id: "beta", name: "Beta", remoteHash: "beta-remote" }]);
  });

  it("keeps a rejected uninstall pending until backups and unmanaged Skills refresh", async () => {
    let releaseInvalidation: (() => void) | undefined;
    const invalidationPending = new Promise<void>((resolve) => {
      releaseInvalidation = resolve;
    });
    uninstallMock.mockRejectedValueOnce(new Error("remove failed"));
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockImplementation(() => invalidationPending);
    const { result } = renderHook(() => useUninstallSkill(), {
      wrapper: createWrapper(queryClient),
    });

    let mutation!: Promise<unknown>;
    act(() => {
      mutation = result.current.mutateAsync("alpha");
      void mutation.catch(() => undefined);
    });

    await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(2));
    expect(result.current.isPending).toBe(true);
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "backups"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "unmanaged"],
    });

    releaseInvalidation?.();
    await act(async () => {
      await expect(mutation).rejects.toThrow("remove failed");
    });
    await waitFor(() => expect(result.current.isPending).toBe(false));
  });

  it("keeps a rejected update pending until backups refresh", async () => {
    let releaseInvalidation: (() => void) | undefined;
    const invalidationPending = new Promise<void>((resolve) => {
      releaseInvalidation = resolve;
    });
    updateSkillMock.mockRejectedValueOnce(new Error("replace failed"));
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockImplementation(() => invalidationPending);
    const { result } = renderHook(() => useUpdateSkill(), {
      wrapper: createWrapper(queryClient),
    });

    let mutation!: Promise<unknown>;
    act(() => {
      mutation = result.current.mutateAsync("alpha");
      void mutation.catch(() => undefined);
    });

    await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(1));
    expect(result.current.isPending).toBe(true);
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "backups"],
    });

    releaseInvalidation?.();
    await act(async () => {
      await expect(mutation).rejects.toThrow("replace failed");
    });
    await waitFor(() => expect(result.current.isPending).toBe(false));
  });

  it("keeps a rejected backup deletion pending until backups refresh", async () => {
    let releaseInvalidation: (() => void) | undefined;
    const invalidationPending = new Promise<void>((resolve) => {
      releaseInvalidation = resolve;
    });
    deleteBackupMock.mockRejectedValueOnce(new Error("partial delete"));
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockImplementation(() => invalidationPending);
    const { result } = renderHook(() => useDeleteSkillBackup(), {
      wrapper: createWrapper(queryClient),
    });

    let mutation!: Promise<unknown>;
    act(() => {
      mutation = result.current.mutateAsync("backup-1");
      void mutation.catch(() => undefined);
    });

    await waitFor(() => expect(invalidateSpy).toHaveBeenCalledTimes(1));
    expect(result.current.isPending).toBe(true);
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["skills", "backups"],
    });

    releaseInvalidation?.();
    await act(async () => {
      await expect(mutation).rejects.toThrow("partial delete");
    });
    await waitFor(() => expect(result.current.isPending).toBe(false));
  });

  it("removes a successfully deleted backup before a refresh failure", async () => {
    deleteBackupMock.mockResolvedValueOnce(true);
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false } },
    });
    queryClient.setQueryData<SkillBackupEntry[]>(
      ["skills", "backups"],
      [
        { backupId: "backup-1" } as SkillBackupEntry,
        { backupId: "backup-2" } as SkillBackupEntry,
      ],
    );
    vi.spyOn(queryClient, "invalidateQueries").mockRejectedValueOnce(
      new Error("refresh failed"),
    );
    const { result } = renderHook(() => useDeleteSkillBackup(), {
      wrapper: createWrapper(queryClient),
    });

    await act(async () => {
      await expect(result.current.mutateAsync("backup-1")).rejects.toThrow(
        "refresh failed",
      );
    });

    expect(
      queryClient.getQueryData<SkillBackupEntry[]>(["skills", "backups"]),
    ).toEqual([{ backupId: "backup-2" }]);
  });
});
