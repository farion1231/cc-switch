import { describe, expect, it } from "vitest";

import { countRepoSkills } from "@/components/skills/RepoManagerPanel";
import type { DiscoverableSkill, SkillRepo } from "@/lib/api/skills";

const repo: SkillRepo = {
  owner: "oil-oil",
  name: "oil-ui",
  branch: "HEAD",
  enabled: true,
};

const skill: DiscoverableSkill = {
  key: "oil-oil/oil-ui:oil-ui",
  name: "oil-ui",
  description: "Design interfaces",
  directory: "oil-ui",
  repoOwner: "oil-oil",
  repoName: "oil-ui",
  repoBranch: "main",
};

describe("countRepoSkills", () => {
  it.each(["main", "master", "develop"])(
    "counts the resolved %s branch for a repository configured with HEAD",
    (repoBranch) => {
      expect(countRepoSkills([{ ...skill, repoBranch }], repo)).toBe(1);
    },
  );

  it("matches GitHub owner and repository names without case sensitivity", () => {
    expect(
      countRepoSkills(
        [{ ...skill, repoOwner: "OIL-OIL", repoName: "Oil-UI" }],
        { ...repo, branch: "main" },
      ),
    ).toBe(1);
  });

  it.each(["", "head", "Head"])(
    "counts the actual default branch for the %j sentinel",
    (branch) => {
      expect(
        countRepoSkills([{ ...skill, repoBranch: "master" }], {
          ...repo,
          branch,
        }),
      ).toBe(1);
    },
  );

  it("does not count skills from another repository for HEAD", () => {
    expect(
      countRepoSkills(
        [
          { ...skill, repoOwner: "other-owner" },
          { ...skill, repoName: "other-repo" },
        ],
        repo,
      ),
    ).toBe(0);
  });

  it("keeps explicit branches distinct and case sensitive", () => {
    expect(
      countRepoSkills(
        [
          skill,
          { ...skill, repoBranch: "develop" },
          { ...skill, repoBranch: "Main" },
          { ...skill, repoBranch: "HEAD" },
        ],
        { ...repo, branch: "main" },
      ),
    ).toBe(1);
  });

  it("preserves the main default for empty branch values", () => {
    expect(
      countRepoSkills([skill, { ...skill, repoBranch: "" }], {
        ...repo,
        branch: "",
      }),
    ).toBe(2);
  });
});
