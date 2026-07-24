import { describe, expect, it } from "vitest";
import type { InstalledSkill } from "@/lib/api/skills";
import { filterInstalledSkills } from "./skillsFilter";

const defaultApps: InstalledSkill["apps"] = {
  claude: false,
  codex: false,
  gemini: false,
  opencode: false,
  openclaw: false,
  hermes: false,
};

function createSkill(
  id: string,
  apps: Partial<InstalledSkill["apps"]> = {},
): InstalledSkill {
  return {
    id,
    name: id,
    directory: id,
    apps: { ...defaultApps, ...apps },
    installedAt: 0,
    updatedAt: 0,
  };
}

describe("filterInstalledSkills", () => {
  const skills = [
    createSkill("claude-skill", { claude: true }),
    createSkill("codex-skill", { codex: true }),
    createSkill("shared-skill", { claude: true, codex: true }),
    createSkill("disabled-skill", { claude: false, codex: false }),
  ];

  it("returns every installed skill for the all filter", () => {
    expect(filterInstalledSkills(skills, "all")).toBe(skills);
  });

  it("returns only skills enabled for the selected app", () => {
    expect(
      filterInstalledSkills(skills, "claude").map((skill) => skill.id),
    ).toEqual(["claude-skill", "shared-skill"]);
    expect(
      filterInstalledSkills(skills, "codex").map((skill) => skill.id),
    ).toEqual(["codex-skill", "shared-skill"]);
  });

  it("does not include skills whose app is disabled or unset", () => {
    expect(filterInstalledSkills(skills, "gemini")).toEqual([]);
  });
});
