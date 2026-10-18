import type { InstalledSkill, SkillProjectGroup } from "@/lib/api/skills";

export interface InstalledSkillGroup {
  key: string;
  label: string;
  project?: SkillProjectGroup;
  skills: InstalledSkill[];
  /** Unassigned local Skills are not one source project; expose individual controls only. */
  bulk: boolean;
}

export const isLocalSkill = (skill: InstalledSkill) =>
  !skill.repoOwner || !skill.repoName;

const sourceKey = (skill: InstalledSkill) =>
  skill.sourceType && skill.sourceId
    ? `source:${skill.sourceType.toLowerCase()}:${skill.sourceId.toLowerCase()}`
    : undefined;

const sourceLabel = (skill: InstalledSkill) => skill.sourceId!;

export function groupInstalledSkills(
  skills: InstalledSkill[],
  projects: SkillProjectGroup[],
  ungroupedLabel: string,
): InstalledSkillGroup[] {
  const groups = new Map<string, InstalledSkillGroup>();
  const assignment = new Map<string, string>();
  for (const project of projects) {
    const key = `project:${project.id}`;
    groups.set(key, {
      key,
      label: project.name,
      project,
      skills: [],
      bulk: true,
    });
    for (const id of project.skillIds) assignment.set(id, key);
  }
  const automaticSourceCounts = new Map<string, number>();
  for (const skill of skills) {
    if (assignment.has(skill.id) || !isLocalSkill(skill)) continue;
    const key = sourceKey(skill);
    if (key)
      automaticSourceCounts.set(key, (automaticSourceCounts.get(key) ?? 0) + 1);
  }

  for (const skill of skills) {
    const repo = !isLocalSkill(skill);
    const automaticSourceKey = sourceKey(skill);
    const key =
      assignment.get(skill.id) ??
      (repo
        ? `repo:${skill.repoOwner!.toLowerCase()}/${skill.repoName!.toLowerCase()}`
        : automaticSourceKey &&
            (automaticSourceCounts.get(automaticSourceKey) ?? 0) > 1
          ? automaticSourceKey
          : "local:ungrouped");
    if (!groups.has(key)) {
      const sourceGrouped = automaticSourceKey === key;
      groups.set(key, {
        key,
        label: repo
          ? `${skill.repoOwner}/${skill.repoName}`
          : sourceGrouped
            ? sourceLabel(skill)
            : ungroupedLabel,
        skills: [],
        bulk: repo || sourceGrouped,
      });
    }
    groups.get(key)!.skills.push(skill);
  }
  return [...groups.values()];
}
