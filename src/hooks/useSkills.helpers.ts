import type {
  DiscoverableSkill,
  InstalledSkill,
  SkillDiscoveryResult,
} from "@/lib/api/skills";

function discoverableInstallName(directory: string): string {
  const trimmed = directory.replace(/[/\\]+$/, "");
  const parts = trimmed.split(/[/\\]/);
  return parts[parts.length - 1] ?? "";
}

function discoverableDedupKey(skill: DiscoverableSkill): string {
  const installName = discoverableInstallName(skill.directory).toLowerCase();
  const uniqueName = installName || skill.key.toLowerCase();
  return `${skill.repoOwner.toLowerCase()}\u0000${skill.repoName.toLowerCase()}\u0000${uniqueName}`;
}

function discoverablePathPriority(
  directory: string,
): [number, number, number, string] {
  const normalized = directory
    .replace(/\\/g, "/")
    .replace(/^\/+|\/+$/g, "");
  const segments = normalized.split("/").filter(Boolean);
  const hasHiddenSegment = segments.some((segment) => segment.startsWith("."))
    ? 1
    : 0;
  const depth = segments.length;
  const notUnderSkillsDir =
    segments[0]?.toLowerCase() === "skills" ? 0 : 1;
  return [hasHiddenSegment, depth, notUnderSkillsDir, normalized.toLowerCase()];
}

function isHigherPriorityPath(candidate: string, existing: string): boolean {
  const a = discoverablePathPriority(candidate);
  const b = discoverablePathPriority(existing);
  for (let i = 0; i < 3; i += 1) {
    if (a[i] !== b[i]) return a[i] < b[i];
  }
  return a[3] < b[3];
}

/**
 * 与后端 `deduplicate_discoverable_skills` 保持一致：
 * 同一仓库内按最终安装目录名去重并优先保留非隐藏、较浅、`skills/` 下的路径；
 * 不同仓库的同名技能分开保留。
 */
export function deduplicateDiscoverableSkills(
  skills: DiscoverableSkill[],
): DiscoverableSkill[] {
  const bestIndexByKey = new Map<string, number>();
  const keep = new Array<boolean>(skills.length).fill(false);

  for (let index = 0; index < skills.length; index += 1) {
    const skill = skills[index];
    const key = discoverableDedupKey(skill);
    const prevIndex = bestIndexByKey.get(key);
    if (prevIndex === undefined) {
      bestIndexByKey.set(key, index);
      keep[index] = true;
    } else if (
      isHigherPriorityPath(skill.directory, skills[prevIndex].directory)
    ) {
      keep[prevIndex] = false;
      keep[index] = true;
      bestIndexByKey.set(key, index);
    }
  }

  return skills.filter((_, index) => keep[index]);
}

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
  const skills = deduplicateDiscoverableSkills([
    ...existing.skills.filter((s) => otherRepo(s.repoOwner, s.repoName)),
    ...(next?.skills ?? []),
  ]).sort((a, b) => {
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
