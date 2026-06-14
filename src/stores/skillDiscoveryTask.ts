import { useSyncExternalStore } from "react";

import type {
  SkillDiscoveryProgress,
  SkillDiscoveryResult,
} from "@/lib/api/skills";

export interface SkillDiscoveryTaskState {
  taskId: string | null;
  active: boolean;
  completed: number;
  total: number;
  repositories: Record<string, SkillDiscoveryProgress>;
}

const initialState: SkillDiscoveryTaskState = {
  taskId: null,
  active: false,
  completed: 0,
  total: 0,
  repositories: {},
};

let state = initialState;
let nextTaskId = 0;
const listeners = new Set<() => void>();
const ignoredRepositories = new Set<string>();

function publish(next: SkillDiscoveryTaskState) {
  state = next;
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getSkillDiscoveryTaskSnapshot() {
  return state;
}

export function useSkillDiscoveryTask() {
  return useSyncExternalStore(
    subscribe,
    getSkillDiscoveryTaskSnapshot,
    getSkillDiscoveryTaskSnapshot,
  );
}

export function beginSkillDiscovery(restart = false) {
  if (!restart && state.active && state.taskId) return state.taskId;
  const taskId = `skill-discovery-${++nextTaskId}`;
  ignoredRepositories.clear();
  publish({
    taskId,
    active: true,
    completed: 0,
    total: 0,
    repositories: {},
  });
  return taskId;
}

export function applySkillDiscoveryProgress(progress: SkillDiscoveryProgress) {
  // 进度事件可能比 invoke 返回更晚到达。任务结束后不再接受旧事件，
  // 避免刷新按钮重新进入加载状态。
  if (
    !state.active ||
    (progress.requestId &&
      state.taskId &&
      progress.requestId !== state.taskId) ||
    ignoredRepositories.has(progress.repo)
  ) {
    return;
  }

  const repositories = {
    ...state.repositories,
    [progress.repo]: progress,
  };
  const adjustedTotal = Math.max(0, progress.total - ignoredRepositories.size);
  publish({
    taskId: state.taskId,
    // 只有 discoverAvailable 的最终返回或异常才能结束任务，避免按钮在
    // 后台仍整理结果时提前恢复可点击。
    active: true,
    completed: Math.min(progress.completed, adjustedTotal),
    total: adjustedTotal,
    repositories,
  });
}

export function removeSkillDiscoveryRepository(repo: string) {
  ignoredRepositories.add(repo);
  if (!(repo in state.repositories) && state.total === 0) return;

  const repositories = { ...state.repositories };
  delete repositories[repo];
  const completed = Object.values(repositories).filter(
    (item) => item.phase === "completed" || item.phase === "failed",
  ).length;
  publish({
    ...state,
    completed,
    total: Math.max(completed, state.total - 1),
    repositories,
  });
}

export function setSkillDiscoveryRepositoryResult(
  repo: string,
  result: SkillDiscoveryResult,
) {
  const failure = result.failures.find(
    (item) => `${item.owner}/${item.name}` === repo,
  );
  const repoSkills = result.skills.filter(
    (skill) => `${skill.repoOwner}/${skill.repoName}` === repo,
  );
  const repositories = {
    ...state.repositories,
    [repo]: failure
      ? {
          phase: "failed" as const,
          completed: state.completed,
          total: state.total,
          repo,
          skillCount: repoSkills.length || undefined,
          error: failure.error,
          skills: repoSkills.length > 0 ? repoSkills : undefined,
        }
      : {
          phase: "completed" as const,
          completed: state.completed,
          total: state.total,
          repo,
          skillCount: repoSkills.length,
          skills: repoSkills,
        },
  };
  const completed = Object.values(repositories).filter(
    (item) => item.phase === "completed" || item.phase === "failed",
  ).length;
  publish({
    ...state,
    completed,
    total: Math.max(state.total, Object.keys(repositories).length),
    repositories,
  });
}

export function finishSkillDiscovery(
  result: SkillDiscoveryResult,
  requestId?: string,
) {
  if (requestId && state.taskId && requestId !== state.taskId) return;
  const repositories = { ...state.repositories };
  const skillsByRepo = new Map<string, typeof result.skills>();

  for (const skill of result.skills) {
    const repo = `${skill.repoOwner}/${skill.repoName}`;
    if (ignoredRepositories.has(repo)) continue;
    const existing = skillsByRepo.get(repo) ?? [];
    existing.push(skill);
    skillsByRepo.set(repo, existing);
  }

  for (const [repo, skills] of skillsByRepo) {
    repositories[repo] = {
      phase: "completed",
      completed: 0,
      total: 0,
      repo,
      skillCount: skills.length,
      skills,
    };
  }

  for (const failure of result.failures) {
    const repo = `${failure.owner}/${failure.name}`;
    if (ignoredRepositories.has(repo)) continue;
    repositories[repo] = {
      phase: "failed",
      completed: 0,
      total: 0,
      repo,
      error: failure.error,
    };
  }

  const total = Math.max(state.total, Object.keys(repositories).length);
  publish({
    taskId: state.taskId,
    active: false,
    completed: total,
    total,
    repositories,
  });
}

export function failSkillDiscovery(requestId?: string) {
  if (requestId && state.taskId && requestId !== state.taskId) return;
  const repositories = Object.fromEntries(
    Object.entries(state.repositories).map(([repo, progress]) => [
      repo,
      progress.phase === "loading" || progress.phase === "scanning"
        ? {
            ...progress,
            phase: "failed" as const,
            error: progress.error ?? "DISCOVERY_FAILED",
          }
        : progress,
    ]),
  );
  const completed = Object.values(repositories).filter(
    (item) => item.phase === "completed" || item.phase === "failed",
  ).length;
  publish({
    ...state,
    active: false,
    completed,
    repositories,
  });
}

export function resetSkillDiscoveryTask() {
  nextTaskId = 0;
  ignoredRepositories.clear();
  publish(initialState);
}
