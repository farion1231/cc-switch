import type { InstalledSkill, SkillDiscoveryResult } from "@/lib/api/skills";

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
  if (imported.length === 0) return existing ?? imported;

  const merged = new Map(existing?.map((skill) => [skill.id, skill]));
  for (const skill of imported) {
    merged.set(skill.id, skill);
  }
  return Array.from(merged.values());
}

/** 下载失败不能把上次成功的发现结果清空；成功的空结果仍然是权威结果。 */
export function mergeDiscoveryResults(
  existing: SkillDiscoveryResult | undefined,
  incoming: SkillDiscoveryResult,
): SkillDiscoveryResult {
  if (!existing || incoming.failures.length === 0) return incoming;

  const retained = existing.skills.filter((skill) =>
    incoming.failures.some(
      (failure) =>
        failure.owner.toLowerCase() === skill.repoOwner.toLowerCase() &&
        failure.name.toLowerCase() === skill.repoName.toLowerCase() &&
        (failure.branch || "main") ===
          ((skill.repoRequestedBranch ?? skill.repoBranch) || "main"),
    ),
  );
  const skills = new Map(
    retained.map((skill) => [skill.key.toLowerCase(), skill]),
  );
  for (const skill of incoming.skills)
    skills.set(skill.key.toLowerCase(), skill);
  return {
    skills: Array.from(skills.values()).sort((a, b) => {
      const x = a.name.toLowerCase();
      const y = b.name.toLowerCase();
      return x < y ? -1 : x > y ? 1 : 0;
    }),
    failures: incoming.failures,
  };
}

/**
 * 把发现结果里某个仓库的 Skill 和失败记录换成 next（不传就是删掉），
 * 去重、排序规则和后端 discover_available_report 一致。
 *
 * existing 为空说明还没有发现结果，返回 undefined 让 setQueryData 不落一份
 * 只含单个仓库的残缺列表（那样 staleTime: Infinity 会让完整发现不再触发）。
 */
export function replaceRepoDiscovery(
  existing: SkillDiscoveryResult | undefined,
  repo: { owner: string; name: string },
  next?: SkillDiscoveryResult,
): SkillDiscoveryResult | undefined {
  if (!existing) return undefined;

  const otherRepo = (owner?: string, name?: string) =>
    owner !== repo.owner || name !== repo.name;
  const replacement = next && mergeDiscoveryResults(existing, next);
  const seen = new Set<string>();
  const skills = [
    ...existing.skills.filter((s) => otherRepo(s.repoOwner, s.repoName)),
    ...(replacement?.skills ?? []),
  ]
    .filter((skill) => {
      const key = skill.key.toLowerCase();
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .sort((a, b) => {
      const x = a.name.toLowerCase();
      const y = b.name.toLowerCase();
      return x < y ? -1 : x > y ? 1 : 0;
    });
  const failures = [
    ...existing.failures.filter((f) => otherRepo(f.owner, f.name)),
    ...(next?.failures ?? []),
  ];
  return { skills, failures };
}
