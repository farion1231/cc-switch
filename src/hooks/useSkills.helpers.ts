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
  const seen = new Set<string>();
  const skills = [
    ...existing.skills.filter((s) => otherRepo(s.repoOwner, s.repoName)),
    ...(next?.skills ?? []),
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
