import { useState, useCallback, useEffect, useRef } from "react";
import {
  skillsApi,
  type Skill,
  type SkillRepo,
  type AppType,
  type RepoLoadingState,
} from "@/lib/api/skills";

/** 本地技能的特殊 key */
export const LOCAL_REPO_KEY = "__local__";

/**
 * 生成仓库的唯一标识 key
 * 格式: "{owner}/{name}"
 */
export function getRepoKey(repo: SkillRepo): string {
  return `${repo.owner}/${repo.name}`;
}

/**
 * 从 owner 和 name 生成仓库 key
 */
export function makeRepoKey(owner: string, name: string): string {
  return `${owner}/${name}`;
}

/**
 * useSkillsLoader Hook 返回值接口
 */
export interface UseSkillsLoaderResult {
  /** 所有已加载的技能列表 */
  skills: Skill[];
  /** 仓库列表 */
  repos: SkillRepo[];
  /** 每个仓库的加载状态，key 为 "{owner}/{name}" */
  repoStates: Map<string, RepoLoadingState>;
  /** 是否有任何仓库正在加载 */
  isLoading: boolean;
  /** 是否正在加载仓库列表 */
  isLoadingRepos: boolean;
  /** 刷新所有仓库的技能 */
  refresh: () => void;
  /** 刷新单个仓库的技能 */
  refreshRepo: (repoKey: string) => void;
}

/**
 * Skills 分仓库渐进式加载 Hook
 *
 * 实现功能：
 * 1. 首先加载仓库列表
 * 2. 立即加载本地技能（不需要网络）
 * 3. 为每个启用的仓库创建独立的加载状态
 * 4. 并行加载每个仓库的技能
 * 5. 每个仓库加载完成后立即更新状态，无需等待其他仓库
 *
 * @param app 应用类型 (claude/codex/gemini)
 * @returns UseSkillsLoaderResult
 */
export function useSkillsLoader(app: AppType): UseSkillsLoaderResult {
  // 仓库列表
  const [repos, setRepos] = useState<SkillRepo[]>([]);
  // 是否正在加载仓库列表
  const [isLoadingRepos, setIsLoadingRepos] = useState(true);
  // 每个仓库的技能，key 为 repoKey
  const [skillsByRepo, setSkillsByRepo] = useState<Map<string, Skill[]>>(
    new Map()
  );
  // 每个仓库的加载状态
  const [repoStates, setRepoStates] = useState<Map<string, RepoLoadingState>>(
    new Map()
  );
  // 已加载的远程技能（用于计算本地独有技能）
  const loadedRemoteSkillsRef = useRef<Skill[]>([]);

  // 用于取消正在进行的加载操作
  const abortControllerRef = useRef<AbortController | null>(null);

  /**
   * 更新单个仓库的加载状态
   */
  const updateRepoState = useCallback(
    (repoKey: string, state: Partial<RepoLoadingState>) => {
      setRepoStates((prev) => {
        const newStates = new Map(prev);
        const currentState = newStates.get(repoKey) || { status: "pending" };
        newStates.set(repoKey, { ...currentState, ...state });
        return newStates;
      });
    },
    []
  );

  /**
   * 更新单个仓库的技能列表
   */
  const updateRepoSkills = useCallback((repoKey: string, skills: Skill[]) => {
    setSkillsByRepo((prev) => {
      const newMap = new Map(prev);
      newMap.set(repoKey, skills);
      return newMap;
    });
  }, []);

  /**
   * 加载本地独有的技能（不需要网络，应该最先完成）
   */
  const loadLocalSkills = useCallback(
    async (remoteSkills: Skill[]) => {
      // 设置状态为 loading
      updateRepoState(LOCAL_REPO_KEY, { status: "loading" });

      try {
        const localSkills = await skillsApi.getLocalSkills(app, remoteSkills);

        // 更新本地技能的状态和列表
        updateRepoState(LOCAL_REPO_KEY, {
          status: "success",
          skillCount: localSkills.length,
          error: undefined,
        });
        updateRepoSkills(LOCAL_REPO_KEY, localSkills);
      } catch (error) {
        console.error("Failed to load local skills:", error);
        const errorMessage =
          error instanceof Error ? error.message : String(error);
        updateRepoState(LOCAL_REPO_KEY, {
          status: "error",
          error: errorMessage,
          skillCount: undefined,
        });
        updateRepoSkills(LOCAL_REPO_KEY, []);
      }
    },
    [app, updateRepoState, updateRepoSkills]
  );

  /**
   * 加载单个仓库的技能
   */
  const loadRepoSkills = useCallback(
    async (repo: SkillRepo): Promise<Skill[]> => {
      const repoKey = getRepoKey(repo);

      // 设置状态为 loading
      updateRepoState(repoKey, { status: "loading" });

      try {
        const skills = await skillsApi.getSkillsForRepo(
          app,
          repo.owner,
          repo.name
        );

        // 加载成功，更新状态和技能列表
        updateRepoState(repoKey, {
          status: "success",
          skillCount: skills.length,
          error: undefined,
        });
        updateRepoSkills(repoKey, skills);

        // 更新已加载的远程技能列表
        loadedRemoteSkillsRef.current = [
          ...loadedRemoteSkillsRef.current,
          ...skills,
        ];

        // 每次远程仓库加载完成后，重新计算本地独有技能
        // 这样可以确保本地技能列表是准确的
        loadLocalSkills(loadedRemoteSkillsRef.current);

        return skills;
      } catch (error) {
        // 加载失败，更新错误状态
        const errorMessage =
          error instanceof Error ? error.message : String(error);
        updateRepoState(repoKey, {
          status: "error",
          error: errorMessage,
          skillCount: undefined,
        });
        // 清空该仓库的技能
        updateRepoSkills(repoKey, []);
        console.error(`Failed to load skills for ${repoKey}:`, error);
        return [];
      }
    },
    [app, updateRepoState, updateRepoSkills, loadLocalSkills]
  );

  /**
   * 加载所有仓库的技能（并行）
   */
  const loadAllRepoSkills = useCallback(
    async (repoList: SkillRepo[]) => {
      // 只加载启用的仓库
      const enabledRepos = repoList.filter((repo) => repo.enabled);

      // 重置已加载的远程技能
      loadedRemoteSkillsRef.current = [];

      // 初始化所有仓库状态为 pending（包括本地）
      const initialStates = new Map<string, RepoLoadingState>();
      initialStates.set(LOCAL_REPO_KEY, { status: "pending" });
      enabledRepos.forEach((repo) => {
        initialStates.set(getRepoKey(repo), { status: "pending" });
      });
      setRepoStates(initialStates);

      // 清空之前的技能数据
      setSkillsByRepo(new Map());

      // 1. 首先加载本地技能（不需要网络，最快）
      // 初始时传入空数组，后续会随着远程仓库加载完成而更新
      await loadLocalSkills([]);

      // 2. 并行加载所有远程仓库的技能
      if (enabledRepos.length > 0) {
        await Promise.all(enabledRepos.map((repo) => loadRepoSkills(repo)));
      }
    },
    [loadRepoSkills, loadLocalSkills]
  );

  /**
   * 加载仓库列表并开始加载技能
   */
  const loadReposAndSkills = useCallback(async () => {
    // 取消之前的加载操作
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
    }
    abortControllerRef.current = new AbortController();

    setIsLoadingRepos(true);

    try {
      const repoList = await skillsApi.getRepos();
      setRepos(repoList);
      setIsLoadingRepos(false);

      // 开始加载技能（本地优先，然后并行加载远程）
      await loadAllRepoSkills(repoList);
    } catch (error) {
      console.error("Failed to load repos:", error);
      setIsLoadingRepos(false);
    }
  }, [loadAllRepoSkills]);

  /**
   * 刷新所有仓库的技能
   */
  const refresh = useCallback(() => {
    loadReposAndSkills();
  }, [loadReposAndSkills]);

  /**
   * 刷新单个仓库的技能
   */
  const refreshRepo = useCallback(
    (repoKey: string) => {
      const repo = repos.find((r) => getRepoKey(r) === repoKey);
      if (repo && repo.enabled) {
        loadRepoSkills(repo);
      }
    },
    [repos, loadRepoSkills]
  );

  // 初始加载
  useEffect(() => {
    loadReposAndSkills();

    // 清理函数
    return () => {
      if (abortControllerRef.current) {
        abortControllerRef.current.abort();
      }
    };
  }, [loadReposAndSkills]);

  // 合并所有仓库的技能为一个列表（本地技能放在前面）
  const skills = [
    ...(skillsByRepo.get(LOCAL_REPO_KEY) || []),
    ...Array.from(skillsByRepo.entries())
      .filter(([key]) => key !== LOCAL_REPO_KEY)
      .flatMap(([, repoSkills]) => repoSkills),
  ];

  // 检查是否有任何仓库正在加载（不包括本地技能）
  const isLoading =
    isLoadingRepos ||
    Array.from(repoStates.entries()).some(
      ([key, state]) =>
        key !== LOCAL_REPO_KEY &&
        (state.status === "pending" || state.status === "loading")
    );

  return {
    skills,
    repos,
    repoStates,
    isLoading,
    isLoadingRepos,
    refresh,
    refreshRepo,
  };
}
