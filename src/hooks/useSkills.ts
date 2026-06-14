import {
  useMutation,
  useQuery,
  useQueryClient,
  keepPreviousData,
  type QueryClient,
} from "@tanstack/react-query";
import {
  skillsApi,
  type SkillBackupEntry,
  type DiscoverableSkill,
  type ImportSkillSelection,
  type InstalledSkill,
  type SkillDiscoveryResult,
  type SkillRepo,
  type SkillUpdateInfo,
  type SkillUpdateCheckResult,
  type SkillsShSearchResult,
} from "@/lib/api/skills";
import type { AppId } from "@/lib/api/types";
import {
  filterSkillDiscoveryResultForRepositories,
  filterSkillUpdatesForInstalled,
  mergeImportedSkills,
  mergeSkillDiscoveryResult,
} from "@/hooks/useSkills.helpers";
import {
  beginSkillDiscovery,
  failSkillDiscovery,
  finishSkillDiscovery,
  removeSkillDiscoveryRepository,
  setSkillDiscoveryRepositoryResult,
} from "@/stores/skillDiscoveryTask";

let skillDiscoveryRequestSequence = 0;
let fullSkillDiscoveryGeneration = 0;
const targetedDiscoveryGenerations = new Map<string, number>();
const successfulFullDiscoverySequences = new Map<string, number>();
const DISCOVERY_QUERY_KEY = ["skills", "discoverable"] as const;
const DISCOVERY_PERSISTED_QUERY_KEY = [
  "skills",
  "discoverable",
  "persisted",
] as const;

function nextSkillDiscoveryRequestSequence() {
  skillDiscoveryRequestSequence += 1;
  return skillDiscoveryRequestSequence;
}

function startFullSkillDiscoveryRequest() {
  fullSkillDiscoveryGeneration += 1;
  const sequence = nextSkillDiscoveryRequestSequence();
  return {
    generation: fullSkillDiscoveryGeneration,
    sequence,
  };
}

function isCurrentFullSkillDiscoveryRequest(generation: number) {
  return generation === fullSkillDiscoveryGeneration;
}

function repositoryIdentity(owner: string, name: string) {
  return `${owner}/${name}`.toLowerCase();
}

function skillRepoIdentity(repo: SkillRepo) {
  return repositoryIdentity(repo.owner, repo.name);
}

function removeSkillRepoFromQueryCaches(
  queryClient: QueryClient,
  removed: { owner: string; name: string },
) {
  const removedRepository = repositoryIdentity(removed.owner, removed.name);
  queryClient.setQueryData<SkillRepo[]>(["skills", "repos"], (current) =>
    current?.filter((repo) => skillRepoIdentity(repo) !== removedRepository),
  );
  const removeRepository = (current: SkillDiscoveryResult | undefined) =>
    current
      ? {
          ...current,
          skills: current.skills.filter(
            (skill) =>
              repositoryIdentity(skill.repoOwner, skill.repoName) !==
              removedRepository,
          ),
          failures: current.failures.filter(
            (failure) =>
              repositoryIdentity(failure.owner, failure.name) !==
              removedRepository,
          ),
          refreshedRepositories: current.refreshedRepositories?.filter(
            (repo) =>
              repositoryIdentity(repo.owner, repo.name) !== removedRepository,
          ),
        }
      : current;
  queryClient.setQueryData<SkillDiscoveryResult>(
    DISCOVERY_QUERY_KEY,
    removeRepository,
  );
  queryClient.setQueryData<SkillDiscoveryResult>(
    DISCOVERY_PERSISTED_QUERY_KEY,
    removeRepository,
  );
}

function startTargetedSkillDiscoveryRequest(repo: SkillRepo) {
  const generation = nextSkillDiscoveryRequestSequence();
  targetedDiscoveryGenerations.set(skillRepoIdentity(repo), generation);
  return generation;
}

function isCurrentTargetedSkillDiscoveryRequest(
  repo: SkillRepo,
  generation: number,
) {
  return (
    targetedDiscoveryGenerations.get(skillRepoIdentity(repo)) === generation &&
    generation >
      (successfulFullDiscoverySequences.get(skillRepoIdentity(repo)) ?? 0)
  );
}

function recordSuccessfulFullDiscoveryRepositories(
  result: SkillDiscoveryResult,
  requestSequence: number,
) {
  const failedRepositories = new Set(
    result.failures.map((failure) =>
      repositoryIdentity(failure.owner, failure.name),
    ),
  );
  const successfulRepositories = new Set(
    (result.refreshedRepositories ?? []).map((repo) =>
      repositoryIdentity(repo.owner, repo.name),
    ),
  );
  for (const skill of result.skills) {
    const repository = repositoryIdentity(skill.repoOwner, skill.repoName);
    if (!failedRepositories.has(repository)) {
      successfulRepositories.add(repository);
    }
  }
  for (const repository of successfulRepositories) {
    successfulFullDiscoverySequences.set(repository, requestSequence);
  }
}

function excludeRepositoriesRefreshedAfter(
  result: SkillDiscoveryResult,
  previous: SkillDiscoveryResult | undefined,
  requestSequence: number,
): SkillDiscoveryResult {
  const isProtected = (owner: string, name: string) =>
    (targetedDiscoveryGenerations.get(repositoryIdentity(owner, name)) ?? 0) >
    requestSequence;
  return {
    ...result,
    skills: result.skills.filter(
      (skill) => !isProtected(skill.repoOwner, skill.repoName),
    ),
    failures: [
      ...result.failures.filter(
        (failure) => !isProtected(failure.owner, failure.name),
      ),
      ...(previous?.failures.filter((failure) =>
        isProtected(failure.owner, failure.name),
      ) ?? []),
    ],
    refreshedRepositories: result.refreshedRepositories?.filter(
      (repo) => !isProtected(repo.owner, repo.name),
    ),
  };
}

async function discoverSkills(queryClient: QueryClient, force = false) {
  const request = startFullSkillDiscoveryRequest();
  const requestId = beginSkillDiscovery(true);
  try {
    const result = await skillsApi.discoverAvailable(force, requestId);
    if (!isCurrentFullSkillDiscoveryRequest(request.generation)) {
      const latest =
        queryClient.getQueryData<SkillDiscoveryResult>(DISCOVERY_QUERY_KEY) ??
        queryClient.getQueryData<SkillDiscoveryResult>(
          DISCOVERY_PERSISTED_QUERY_KEY,
        ) ??
        result;
      finishSkillDiscovery(latest, requestId);
      return latest;
    }
    recordSuccessfulFullDiscoveryRepositories(result, request.sequence);
    const previous =
      queryClient.getQueryData<SkillDiscoveryResult>(DISCOVERY_QUERY_KEY) ??
      queryClient.getQueryData<SkillDiscoveryResult>(
        DISCOVERY_PERSISTED_QUERY_KEY,
      );
    const eligibleResult = excludeRepositoriesRefreshedAfter(
      result,
      previous,
      request.sequence,
    );
    const merged = mergeSkillDiscoveryResult(previous, eligibleResult);
    const current = filterSkillDiscoveryResultForRepositories(
      merged,
      queryClient.getQueryData<SkillRepo[]>(["skills", "repos"]),
    );
    finishSkillDiscovery(current, requestId);
    return current;
  } catch (error) {
    failSkillDiscovery(requestId);
    throw error;
  }
}

async function checkSkillUpdates(queryClient: QueryClient, force = false) {
  const installedAtStart = queryClient.getQueryData<InstalledSkill[]>([
    "skills",
    "installed",
  ]);
  const result = await skillsApi.checkUpdates(force);
  return filterSkillUpdatesForInstalled(
    result,
    queryClient.getQueryData<InstalledSkill[]>(["skills", "installed"]),
    installedAtStart,
  );
}

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
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills", "backups"] });
    },
  });
}

/**
 * 发现可安装的 Skills（从仓库获取）
 * 使用 staleTime: Infinity 和 placeholderData: keepPreviousData
 * 实现首次进入使用缓存，只有刷新时才重新获取
 */
export function useDiscoverableSkills() {
  const queryClient = useQueryClient();
  const persistedQuery = useQuery({
    queryKey: DISCOVERY_PERSISTED_QUERY_KEY,
    queryFn: () => skillsApi.getCachedDiscoverable(),
    staleTime: Infinity,
  });
  const query = useQuery({
    queryKey: DISCOVERY_QUERY_KEY,
    queryFn: () => discoverSkills(queryClient),
    enabled: persistedQuery.isSuccess,
    staleTime: Infinity,
    placeholderData: keepPreviousData,
  });
  const forceMutation = useMutation({
    mutationFn: () => discoverSkills(queryClient, true),
    onSuccess: (result) => {
      queryClient.setQueryData(DISCOVERY_QUERY_KEY, result);
    },
  });
  return {
    ...query,
    data: query.data ?? persistedQuery.data,
    isLoading: persistedQuery.isLoading && !persistedQuery.data,
    isFetching:
      persistedQuery.isFetching || query.isFetching || forceMutation.isPending,
    forceRefetch: async () => ({
      data: await forceMutation.mutateAsync(),
    }),
  };
}

/**
 * 只重新加载一个仓库，并将结果合并到发现列表缓存。
 */
export function useRetrySkillRepo() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (repo: SkillRepo) => skillsApi.discoverRepo(repo),
    onMutate: (repo) => ({
      generation: startTargetedSkillDiscoveryRequest(repo),
    }),
    onSuccess: (result, repo, context) => {
      if (!context) return;
      if (!isCurrentTargetedSkillDiscoveryRequest(repo, context.generation)) {
        return;
      }
      setSkillDiscoveryRepositoryResult(`${repo.owner}/${repo.name}`, result);
      queryClient.setQueryData<SkillDiscoveryResult>(
        DISCOVERY_QUERY_KEY,
        (oldData) => {
          const baseData =
            oldData ??
            queryClient.getQueryData<SkillDiscoveryResult>(
              DISCOVERY_PERSISTED_QUERY_KEY,
            );
          if (!baseData) return result;

          const merged = mergeSkillDiscoveryResult(baseData, result);

          return {
            ...merged,
            skills: [...merged.skills].sort((a, b) =>
              a.name.localeCompare(b.name),
            ),
            failures: [
              ...baseData.failures.filter(
                (failure) =>
                  repositoryIdentity(failure.owner, failure.name) !==
                  skillRepoIdentity(repo),
              ),
              ...result.failures,
            ],
          };
        },
      );
    },
  });
}

/**
 * 安装 Skill
 * 成功后直接更新缓存，不触发重新加载/刷新
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
    onSuccess: (installedSkill, _vars, _ctx) => {
      const { skill } = _vars;
      // 直接更新 installed 缓存
      queryClient.setQueryData<InstalledSkill[]>(
        ["skills", "installed"],
        (oldData) => {
          if (!oldData) return [installedSkill];
          return [...oldData, installedSkill];
        },
      );

      // 更新 discoverable 缓存中对应技能的 installed 状态
      const installName =
        skill.directory.split(/[/\\]/).pop()?.toLowerCase() ||
        skill.directory.toLowerCase();
      const skillKey = `${installName}:${skill.repoOwner.toLowerCase()}:${skill.repoName.toLowerCase()}`;

      queryClient.setQueryData<SkillDiscoveryResult>(
        DISCOVERY_QUERY_KEY,
        (oldData) => {
          if (!oldData) return oldData;
          return {
            ...oldData,
            skills: oldData.skills.map((s) => {
              if (s.key === skillKey) {
                return { ...s, installed: true };
              }
              return s;
            }),
          };
        },
      );
    },
  });
}

/**
 * 卸载 Skill
 * 成功后直接更新缓存，不触发重新加载/刷新
 */
export function useUninstallSkill() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, skillKey }: { id: string; skillKey: string }) =>
      skillsApi
        .uninstallUnified(id)
        .then((result) => ({ ...result, skillKey })),
    onSuccess: ({ skillKey }, _vars) => {
      const installedBefore = queryClient.getQueryData<InstalledSkill[]>([
        "skills",
        "installed",
      ]);
      const remainingInstalled = installedBefore?.filter(
        (s) => s.id !== _vars.id,
      );
      // 直接更新 installed 缓存，移除该 skill
      queryClient.setQueryData<InstalledSkill[]>(
        ["skills", "installed"],
        (oldData) => (oldData ? remainingInstalled : oldData),
      );

      // 更新 discoverable 缓存中对应技能的 installed 状态
      queryClient.setQueryData<SkillDiscoveryResult>(
        DISCOVERY_QUERY_KEY,
        (oldData) => {
          if (!oldData) return oldData;
          return {
            ...oldData,
            skills: oldData.skills.map((s) => {
              if (s.key === skillKey) {
                return { ...s, installed: false };
              }
              return s;
            }),
          };
        },
      );
      queryClient.setQueryData<SkillUpdateCheckResult>(
        ["skills", "updates"],
        (oldData) =>
          oldData && remainingInstalled
            ? filterSkillUpdatesForInstalled(oldData, remainingInstalled)
            : oldData
              ? {
                  ...oldData,
                  updates: oldData.updates.filter(
                    (update) => update.id !== _vars.id,
                  ),
                }
              : oldData,
      );
    },
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
    onSuccess: (restoredSkill) => {
      queryClient.setQueryData<InstalledSkill[]>(
        ["skills", "installed"],
        (oldData) => {
          if (!oldData) return [restoredSkill];
          const existing = oldData.some(
            (skill) => skill.id === restoredSkill.id,
          );
          return existing
            ? oldData.map((skill) =>
                skill.id === restoredSkill.id ? restoredSkill : skill,
              )
            : [...oldData, restoredSkill];
        },
      );
      queryClient.setQueryData<SkillUpdateCheckResult>(
        ["skills", "updates"],
        (oldData) =>
          oldData
            ? {
                ...oldData,
                updates: oldData.updates.filter(
                  (update) => update.id !== restoredSkill.id,
                ),
              }
            : oldData,
      );
      queryClient.invalidateQueries({ queryKey: ["skills", "backups"] });
    },
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
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["skills", "installed"] });
    },
  });
}

/**
 * 扫描未管理的 Skills
 */
export function useScanUnmanagedSkills() {
  return useQuery({
    queryKey: ["skills", "unmanaged"],
    queryFn: () => skillsApi.scanUnmanaged(),
    enabled: false, // 手动触发
  });
}

/**
 * 从应用目录导入 Skills
 * 成功后直接更新缓存，不触发重新加载/刷新
 */
export function useImportSkillsFromApps() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (imports: ImportSkillSelection[]) =>
      skillsApi.importFromApps(imports),
    onSuccess: (importedSkills) => {
      // 直接更新 installed 缓存
      queryClient.setQueryData<InstalledSkill[]>(
        ["skills", "installed"],
        (oldData) => mergeImportedSkills(oldData, importedSkills),
      );
      // 刷新 unmanaged 列表（已被导入的应该移除）
      queryClient.invalidateQueries({ queryKey: ["skills", "unmanaged"] });
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
    onSuccess: (_result, repo) => {
      queryClient.setQueryData<SkillRepo[]>(
        ["skills", "repos"],
        (current = []) => {
          const repository = skillRepoIdentity(repo);
          const existingIndex = current.findIndex(
            (item) => skillRepoIdentity(item) === repository,
          );
          if (existingIndex === -1) {
            return [...current, repo];
          }
          return current.map((item, index) =>
            index === existingIndex ? repo : item,
          );
        },
      );
      queryClient.invalidateQueries({ queryKey: ["skills", "repos"] });
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
    onMutate: async (removed) => {
      await Promise.all([
        queryClient.cancelQueries({ queryKey: ["skills", "repos"] }),
        queryClient.cancelQueries({ queryKey: DISCOVERY_QUERY_KEY }),
        queryClient.cancelQueries({ queryKey: DISCOVERY_PERSISTED_QUERY_KEY }),
      ]);
      const previousRepos = queryClient.getQueryData<SkillRepo[]>([
        "skills",
        "repos",
      ]);
      const previousDiscovery =
        queryClient.getQueryData<SkillDiscoveryResult>(DISCOVERY_QUERY_KEY);
      const previousPersistedDiscovery =
        queryClient.getQueryData<SkillDiscoveryResult>(
          DISCOVERY_PERSISTED_QUERY_KEY,
        );
      removeSkillRepoFromQueryCaches(queryClient, removed);
      return {
        previousRepos,
        previousDiscovery,
        previousPersistedDiscovery,
      };
    },
    onError: (_error, _removed, context) => {
      if (!context) return;
      queryClient.setQueryData(["skills", "repos"], context?.previousRepos);
      queryClient.setQueryData(DISCOVERY_QUERY_KEY, context?.previousDiscovery);
      queryClient.setQueryData(
        DISCOVERY_PERSISTED_QUERY_KEY,
        context?.previousPersistedDiscovery,
      );
    },
    onSuccess: (_result, removed) => {
      const repository = repositoryIdentity(removed.owner, removed.name);
      targetedDiscoveryGenerations.delete(repository);
      successfulFullDiscoverySequences.delete(repository);
      removeSkillDiscoveryRepository(`${removed.owner}/${removed.name}`);
      removeSkillRepoFromQueryCaches(queryClient, removed);
      queryClient.invalidateQueries({ queryKey: ["skills", "repos"] });
    },
  });
}

/**
 * 从 ZIP 文件安装 Skills
 * 成功后直接更新缓存，不触发重新加载/刷新
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
    onSuccess: (installedSkills) => {
      // 直接更新 installed 缓存
      queryClient.setQueryData<InstalledSkill[]>(
        ["skills", "installed"],
        (oldData) => {
          if (!oldData) return installedSkills;
          return [...oldData, ...installedSkills];
        },
      );
    },
  });
}

// ========== 更新检测 ==========

/**
 * 检查 Skills 更新（手动触发）
 */
export function useCheckSkillUpdates() {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: ["skills", "updates"],
    queryFn: () => checkSkillUpdates(queryClient),
    enabled: false,
    staleTime: 5 * 60 * 1000,
  });
  const forceMutation = useMutation({
    mutationFn: () => checkSkillUpdates(queryClient, true),
    onSuccess: (result) => {
      queryClient.setQueryData(["skills", "updates"], result);
    },
  });
  return {
    ...query,
    isFetching: query.isFetching || forceMutation.isPending,
    forceRefetch: async () => ({
      data: await forceMutation.mutateAsync(),
    }),
  };
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
