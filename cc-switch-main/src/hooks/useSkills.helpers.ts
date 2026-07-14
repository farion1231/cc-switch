import type { InstalledSkill } from "@/lib/api/skills";

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
