import {
  useMutation,
  useQuery,
  useQueryClient,
  keepPreviousData,
} from "@tanstack/react-query";
import {
  skillsApi,
  type SkillBackupEntry,
  type DiscoverableSkill,
  type ImportSkillSelection,
  type InstalledSkill,
  type SkillDiscoveryResult,
  type SkillUpdateCheckResult,
  type SkillUpdateInfo,
  type SkillsShSearchResult,
} from "@/lib/api/skills";
import type { AppId } from "@/lib/api/types";
import { mergeImportedSkills } from "@/hooks/useSkills.helpers";
import { readLocalCache, writeLocalCache } from "@/lib/localCache";
import { runSequentialBulkAction } from "@/lib/utils/sequentialBulkAction";

/**
 * 查询所有已安装的 Skills
 * 使用 staleTime: Infinity 和 placeholderData: keepPreviousData
 * 实现首次进入使用缓存，只有刷新时才重新获取
 */
export function useInstalledSkills() {
  return useQuery({
    queryKey: ["skills", "installed"],
    queryFn: () => skillsApi.getInstalled(),
    staleTime: Infinity,
    placeholderData: keepPreviousData,
  });
}

export function useSkillBackups() {
  return useQuery({
    queryKey: ["skills", "backups"],
    queryFn: () => skillsApi.getBackups(),
    enabled: false,
  });
}

export function useDeleteSkillBackup() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (backupId: string) => skillsApi.deleteBackup(backupId),
    onSuccess: (_result, backupId) => {
      queryClient.setQueryData<SkillBackupEntry[]>(
        ["skills", "backups"],
        (oldData) => oldData?.filter((backup) => backup.backupId !== backupId),
      );
    },
    // remove_dir_all can partially change the backup directory before
    // returning an error, so reconcile the authoritative list either way.
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: ["skills", "backups"] }),
  });
}

/**
 * 发现可安装的 Skills（从仓库获取）
 * 使用 staleTime: Infinity 和 placeholderData: keepPreviousData
 * 实现首次进入使用缓存，只有刷新时才重新获取
 */
// 发现要从 GitHub 下载每个仓库，很慢：上次的结果存在本地，打开时先显示，
// 每次启动只在后台重新拉一次（之后同一次运行里不再自动拉，手动刷新照常）
const DISCOVER_CACHE_KEY = "skills.discoverable.v1";
let discoverRevalidated = false;

const discoverableQuery = {
  queryKey: ["skills", "discoverable"],
  queryFn: async () => {
    const result = await skillsApi.discoverAvailable();
    discoverRevalidated = true;
    writeLocalCache(DISCOVER_CACHE_KEY, result);
    return result;
  },
  staleTime: Infinity,
  initialData: () => readLocalCache<SkillDiscoveryResult>(DISCOVER_CACHE_KEY),
  // 本地缓存是旧数据：时间记成 0，配合 refetchOnMount 在后台刷新
  initialDataUpdatedAt: 0,
  refetchOnMount: () => (discoverRevalidated ? false : ("always" as const)),
  placeholderData: keepPreviousData,
};

export function useDiscoverableSkills() {
  return useQuery({ ...discoverableQuery, select: selectDiscoveredSkills });
}

const selectDiscoveredSkills = (result: SkillDiscoveryResult) => result.skills;
const selectDiscoveryFailures = (result: SkillDiscoveryResult) =>
  result.failures;

/**
 * 发现时没读到的仓库（和 useDiscoverableSkills 共用一次请求）
 */
export function useDiscoverableSkillsFailures() {
  return useQuery({ ...discoverableQuery, select: selectDiscoveryFailures });
}

/**
 * 安装 Skill
 * 成功后先合并缓存，并在结束后刷新权威列表
 */
export function useInstallSkill() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      skill,
      currentApp,
    }: {
      skill: DiscoverableSkill;
      currentApp: AppId;
    }) => skillsApi.installUnified(skill, currentApp),
    onSuccess: (installedSkill) => {
      queryClient.setQueryData<InstalledSkill[]>(
        ["skills", "installed"],
        (oldData) => mergeImportedSkills(oldData, [installedSkill]),
      );
    },
    // The backend can persist the installation before live-config sync fails.
    // Always refresh the authoritative list, including rejected mutations.
    onSettled: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skills", "installed"] }),
        queryClient.invalidateQueries({ queryKey: ["skills", "unmanaged"] }),
      ]),
  });
}

/**
 * 卸载 Skill
 * 成功后直接移除已安装缓存，并在结束后收敛备份与未管理列表
 */
export function useUninstallSkill() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => skillsApi.uninstallUnified(id),
    onSuccess: (_result, id) => {
      // 直接更新 installed 缓存，移除该 skill
      queryClient.setQueryData<InstalledSkill[]>(
        ["skills", "installed"],
        (oldData) => {
          if (!oldData) return oldData;
          return oldData.filter((s) => s.id !== id);
        },
      );

      // A completed update check may still contain this Skill. Remove it so
      // Update All cannot target an ID that was just uninstalled.
      queryClient.setQueryData<SkillUpdateCheckResult>(
        ["skills", "updates"],
        (oldData) =>
          oldData && {
            ...oldData,
            updates: oldData.updates.filter((update) => update.id !== id),
          },
      );
    },
    // Uninstall creates a backup before removing SSOT/DB state. It may reject
    // after that backup exists, and best-effort app cleanup can also leave an
    // unmanaged copy after a successful uninstall.
    onSettled: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skills", "backups"] }),
        queryClient.invalidateQueries({ queryKey: ["skills", "unmanaged"] }),
      ]),
  });
}

export function useRestoreSkillBackup() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      backupId,
      currentApp,
    }: {
      backupId: string;
      currentApp: AppId;
    }) => skillsApi.restoreBackup(backupId, currentApp),
    onSettled: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skills", "installed"] }),
        queryClient.invalidateQueries({ queryKey: ["skills", "backups"] }),
      ]),
  });
}

/**
 * 切换 Skill 在特定应用的启用状态
 */
export function useToggleSkillApp() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      app,
      enabled,
    }: {
      id: string;
      app: AppId;
      enabled: boolean;
    }) => skillsApi.toggleApp(id, app, enabled),
    // 乐观更新：点了格子立刻翻过来，不等写完再刷新；失败时回滚
    onMutate: async ({ id, app, enabled }) => {
      await queryClient.cancelQueries({ queryKey: ["skills", "installed"] });
      const previous = queryClient.getQueryData<InstalledSkill[]>([
        "skills",
        "installed",
      ]);
      if (previous) {
        queryClient.setQueryData<InstalledSkill[]>(
          ["skills", "installed"],
          previous.map((skill) =>
            skill.id === id
              ? { ...skill, apps: { ...skill.apps, [app]: enabled } }
              : skill,
          ),
        );
      }
      return { previous };
    },
    onError: (_error, _vars, context) => {
      if (context?.previous) {
        queryClient.setQueryData(["skills", "installed"], context.previous);
      }
    },
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: ["skills", "installed"] }),
  });
}

/** Toggle multiple Skills serially because each operation writes app files. */
export function useBulkToggleSkillApp() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      ids,
      app,
      enabled,
    }: {
      ids: string[];
      app: AppId;
      enabled: boolean;
    }) =>
      runSequentialBulkAction(ids, (id) =>
        skillsApi.toggleApp(id, app, enabled),
      ),
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: ["skills", "installed"] }),
  });
}

/**
 * 扫描未管理的 Skills
 *
 * - 传 { enabled: true }（Skill 面板挂载时）会在进入页面时自动静默扫描一次，
 *   30s 内复用结果，避免来回切页时重复磁盘 IO。
 * - 默认 enabled: false：仅订阅共享缓存（如顶栏「导入」按钮的绿点提示），
 *   不主动触发扫描。两者共用同一 queryKey，面板扫描完成后绿点会自动亮起。
 */
export function useScanUnmanagedSkills(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ["skills", "unmanaged"],
    queryFn: () => skillsApi.scanUnmanaged(),
    enabled: options?.enabled ?? false,
    staleTime: 30 * 1000,
    placeholderData: keepPreviousData,
  });
}

/**
 * 从应用目录导入 Skills
 * 成功后先合并缓存，并在结束后刷新所有可能受影响的列表
 */
export function useImportSkillsFromApps() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (imports: ImportSkillSelection[]) =>
      skillsApi.importFromApps(imports),
    onSuccess: (importedSkills) => {
      queryClient.setQueryData<InstalledSkill[]>(
        ["skills", "installed"],
        (oldData) => mergeImportedSkills(oldData, importedSkills),
      );
    },
    // Import may persist Skills or auto-discovered repositories before a
    // later item fails, so refresh every affected authoritative collection.
    // 只等导入弹窗关心的两个列表：mutation 要等 onSettled 返回的 Promise
    // 才结束，「发现」重拉要从 GitHub 下载仓库，等它会让弹窗一直锁着（#7994）
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: ["skills", "repos"] });
      void queryClient.invalidateQueries({
        queryKey: ["skills", "discoverable"],
      });
      return Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skills", "installed"] }),
        queryClient.invalidateQueries({ queryKey: ["skills", "unmanaged"] }),
      ]);
    },
  });
}

/**
 * 获取仓库列表
 */
export function useSkillRepos() {
  return useQuery({
    queryKey: ["skills", "repos"],
    queryFn: () => skillsApi.getRepos(),
  });
}

/**
 * 添加仓库
 */
export function useAddSkillRepo() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: skillsApi.addRepo,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills", "repos"] });
      queryClient.invalidateQueries({ queryKey: ["skills", "discoverable"] });
    },
  });
}

/**
 * 删除仓库
 */
export function useRemoveSkillRepo() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ owner, name }: { owner: string; name: string }) =>
      skillsApi.removeRepo(owner, name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills", "repos"] });
      queryClient.invalidateQueries({ queryKey: ["skills", "discoverable"] });
    },
  });
}

/**
 * 从 ZIP 文件安装 Skills
 * 成功后先合并缓存，并在结束后刷新权威列表
 */
export function useInstallSkillsFromZip() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      filePath,
      currentApp,
    }: {
      filePath: string;
      currentApp: AppId;
    }) => skillsApi.installFromZip(filePath, currentApp),
    onSuccess: (result) => {
      queryClient.setQueryData<InstalledSkill[]>(
        ["skills", "installed"],
        (oldData) => mergeImportedSkills(oldData, result.installed),
      );
    },
    // A ZIP can install multiple Skills before a later item or config sync
    // fails, so refresh even when the mutation rejects.
    onSettled: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skills", "installed"] }),
        queryClient.invalidateQueries({ queryKey: ["skills", "unmanaged"] }),
      ]),
  });
}

// ========== 更新检测 ==========

/**
 * 检查 Skills 更新（手动触发）；结果里带着没读到的仓库
 */
export function useCheckSkillUpdates() {
  return useQuery({
    queryKey: ["skills", "updates"],
    queryFn: () => skillsApi.checkUpdates(),
    enabled: false,
    staleTime: 5 * 60 * 1000,
  });
}

/**
 * 更新单个 Skill
 */
export function useUpdateSkill() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => skillsApi.updateSkill(id),
    onSuccess: (updatedSkill) => {
      queryClient.setQueryData<InstalledSkill[]>(
        ["skills", "installed"],
        (oldData) => {
          if (!oldData) return [updatedSkill];
          return oldData.map((s) =>
            s.id === updatedSkill.id ? updatedSkill : s,
          );
        },
      );
      queryClient.setQueryData<SkillUpdateCheckResult>(
        ["skills", "updates"],
        (oldData) => {
          if (!oldData) return oldData;
          return {
            ...oldData,
            updates: oldData.updates.filter((u) => u.id !== updatedSkill.id),
          };
        },
      );
    },
    // Updating creates an uninstall-style backup before replacing SSOT files;
    // refresh even when replacement or persistence fails later.
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: ["skills", "backups"] }),
  });
}

// ========== skills.sh 搜索 ==========

/**
 * 搜索 skills.sh 公共目录
 * 使用 300ms staleTime 和 keepPreviousData 实现平滑搜索体验
 */
export function useSearchSkillsSh(
  query: string,
  limit: number,
  offset: number,
) {
  return useQuery({
    queryKey: ["skills", "skillssh", query, limit, offset],
    queryFn: () => skillsApi.searchSkillsSh(query, limit, offset),
    enabled: query.length >= 2,
    staleTime: 5 * 60 * 1000,
    placeholderData: keepPreviousData,
  });
}

// ========== 辅助类型 ==========

export type {
  InstalledSkill,
  DiscoverableSkill,
  ImportSkillSelection,
  SkillBackupEntry,
  SkillUpdateInfo,
  SkillsShSearchResult,
  AppId,
};

/**
 * 立即重新同步：按开关和当前同步方式把 Skill 重新投影到各应用目录
 */
export function useResyncSkillsToApps() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => skillsApi.resyncToApps(),
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: ["skills", "installed"] }),
  });
}

/**
 * CC Switch 目录下放 Skill 主副本的真实路径（改过配置目录就是改后的）
 */
export function useCcSwitchSkillsDir(enabled = true) {
  return useQuery({
    queryKey: ["skills", "ccSwitchDir"],
    queryFn: () => skillsApi.getCcSwitchSkillsDir(),
    enabled,
    staleTime: Infinity,
  });
}
