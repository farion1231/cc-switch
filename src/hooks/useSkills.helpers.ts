import type {
  InstalledSkill,
  SkillDiscoveryResult,
  SkillRepo,
  SkillUpdateCheckResult,
} from "@/lib/api/skills";

const repositoryIdentity = (owner: string, name: string) =>
  `${owner}/${name}`.toLowerCase();

/**
 * 合并本次导入结果到已安装缓存，按 id 去重。
 *
 * 同一技能重复出现时以新记录为准，避免 mutation 被重复触发时
 * 在 installed 列表中看到重复条目。imported 为空时返回原引用，
 * 让 React Query 跳过无谓的订阅者通知。
 */
export function mergeImportedSkills(
  existing: InstalledSkill[] | undefined,
  imported: InstalledSkill[],
): InstalledSkill[] {
  if (!existing) return imported;
  if (imported.length === 0) return existing;
  const importedIds = new Set(imported.map((s) => s.id));
  const preserved = existing.filter((s) => !importedIds.has(s.id));
  return [...preserved, ...imported];
}

/**
 * 合并发现页刷新结果。
 *
 * 刷新失败只说明本次无法验证仓库，并不代表仓库里已经没有技能。
 * 因此失败仓库继续展示上一次成功识别的技能；成功仓库完全采用新结果。
 */
export function mergeSkillDiscoveryResult(
  previous: SkillDiscoveryResult | undefined,
  next: SkillDiscoveryResult,
): SkillDiscoveryResult {
  if (!previous) return next;

  const failedRepositories = new Set(
    next.failures.map((failure) =>
      repositoryIdentity(failure.owner, failure.name),
    ),
  );
  const refreshedRepositories = new Set(
    (next.refreshedRepositories ?? []).map((repo) =>
      repositoryIdentity(repo.owner, repo.name),
    ),
  );
  const unverifiedFailedRepositories = new Set(
    [...failedRepositories].filter(
      (repository) => !refreshedRepositories.has(repository),
    ),
  );
  const attemptedRepositories = new Set([
    ...next.skills.map((skill) =>
      repositoryIdentity(skill.repoOwner, skill.repoName),
    ),
    ...refreshedRepositories,
    ...failedRepositories,
  ]);
  const previousKnownRepositories = new Set([
    ...previous.skills.map((skill) =>
      repositoryIdentity(skill.repoOwner, skill.repoName),
    ),
    ...(previous.refreshedRepositories ?? []).map((repo) =>
      repositoryIdentity(repo.owner, repo.name),
    ),
  ]);
  const retainedSkills = previous.skills.filter((skill) => {
    const repository = repositoryIdentity(skill.repoOwner, skill.repoName);
    return (
      unverifiedFailedRepositories.has(repository) ||
      !attemptedRepositories.has(repository)
    );
  });
  const eligibleNextSkills = next.skills.filter((skill) => {
    const repository = repositoryIdentity(skill.repoOwner, skill.repoName);
    return !(
      unverifiedFailedRepositories.has(repository) &&
      previousKnownRepositories.has(repository)
    );
  });
  const nextKeys = new Set(eligibleNextSkills.map((skill) => skill.key));
  const mergedRefreshedRepositories = new Map(
    (previous.refreshedRepositories ?? []).map((repo) => [
      repositoryIdentity(repo.owner, repo.name),
      repo,
    ]),
  );
  for (const repo of next.refreshedRepositories ?? []) {
    mergedRefreshedRepositories.set(
      repositoryIdentity(repo.owner, repo.name),
      repo,
    );
  }

  return {
    ...next,
    skills: [
      ...retainedSkills.filter((skill) => !nextKeys.has(skill.key)),
      ...eligibleNextSkills,
    ],
    failures: next.failures,
    refreshedRepositories:
      mergedRefreshedRepositories.size > 0
        ? [...mergedRefreshedRepositories.values()]
        : undefined,
  };
}

/**
 * 丢弃刷新期间已被用户删除的仓库结果，防止迟到的请求重新写回卡片。
 */
export function filterSkillDiscoveryResultForRepositories(
  result: SkillDiscoveryResult,
  repositories: SkillRepo[] | undefined,
): SkillDiscoveryResult {
  if (!repositories) return result;
  const configured = new Set(
    repositories.map((repo) => repositoryIdentity(repo.owner, repo.name)),
  );
  return {
    ...result,
    skills: result.skills.filter((skill) =>
      configured.has(repositoryIdentity(skill.repoOwner, skill.repoName)),
    ),
    failures: result.failures.filter((failure) =>
      configured.has(repositoryIdentity(failure.owner, failure.name)),
    ),
    refreshedRepositories: result.refreshedRepositories?.filter((repo) =>
      configured.has(repositoryIdentity(repo.owner, repo.name)),
    ),
  };
}

/**
 * 更新检查可能与卸载操作并行完成。只保留当前仍安装的 Skill，
 * 防止旧检查结果重新带回已经删除的更新项。
 */
export function filterSkillUpdatesForInstalled(
  result: SkillUpdateCheckResult,
  installed: InstalledSkill[] | undefined,
  installedAtStart?: InstalledSkill[],
): SkillUpdateCheckResult {
  if (!installed) return result;
  const currentById = new Map(installed.map((skill) => [skill.id, skill]));
  const currentRepositories = new Set(
    installed.flatMap((skill) =>
      skill.repoOwner && skill.repoName
        ? [repositoryIdentity(skill.repoOwner, skill.repoName)]
        : [],
    ),
  );
  const initialById = installedAtStart
    ? new Map(installedAtStart.map((skill) => [skill.id, skill]))
    : undefined;
  const isSameSkillSnapshot = (
    current: InstalledSkill,
    initial: InstalledSkill,
  ) =>
    current.directory === initial.directory &&
    current.repoOwner === initial.repoOwner &&
    current.repoName === initial.repoName &&
    current.repoBranch === initial.repoBranch &&
    current.installedAt === initial.installedAt &&
    current.updatedAt === initial.updatedAt &&
    current.contentHash === initial.contentHash;

  return {
    ...result,
    updates: result.updates.filter((update) => {
      const current = currentById.get(update.id);
      if (!current) return false;
      if (!initialById) return true;
      const initial = initialById.get(update.id);
      return initial ? isSameSkillSnapshot(current, initial) : false;
    }),
    failures: result.failures.filter((failure) =>
      currentRepositories.has(repositoryIdentity(failure.owner, failure.name)),
    ),
  };
}
