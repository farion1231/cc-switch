import type { AppId } from "@/lib/api/types";
import type { InstalledSkill } from "@/lib/api/skills";

export type SkillsAppFilter = AppId | "all";

export function filterInstalledSkills(
  skills: InstalledSkill[],
  filter: SkillsAppFilter,
): InstalledSkill[] {
  if (filter === "all") return skills;

  return skills.filter((skill) => skill.apps[filter] === true);
}
