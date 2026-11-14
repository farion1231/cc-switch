import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  RepoManagerPanel,
  countRepoSkills,
} from "@/components/skills/RepoManagerPanel";
import type { DiscoverableSkill, SkillRepo } from "@/lib/api/skills";

function skill(branch: string, name = "s"): DiscoverableSkill {
  return {
    key: `o/r:${name}`,
    name,
    description: "",
    directory: name,
    repoOwner: "o",
    repoName: "r",
    repoBranch: branch,
  };
}

const repo = (branch: string): SkillRepo => ({
  owner: "o",
  name: "r",
  branch,
  enabled: true,
});

// #9273：发现结果记的是实际下载成功的分支，配置的分支可能是 HEAD 或回落前的分支
describe("countRepoSkills", () => {
  it("counts skills resolved from the default-branch sentinel", () => {
    const skills = [skill("main", "a"), skill("main", "b")];
    expect(countRepoSkills(skills, repo("HEAD"))).toBe(2);
    expect(countRepoSkills(skills, repo("head"))).toBe(2);
    expect(countRepoSkills(skills, repo(""))).toBe(2);
    expect(countRepoSkills([skill("master")], repo("HEAD"))).toBe(1);
  });

  it("counts skills when the configured branch fell back to master", () => {
    expect(countRepoSkills([skill("master")], repo("main"))).toBe(1);
  });

  it("does not count skills from a branch discovery could not have used", () => {
    expect(countRepoSkills([skill("feature")], repo("HEAD"))).toBe(0);
    expect(countRepoSkills([skill("dev")], repo("main"))).toBe(0);
    expect(countRepoSkills([skill("dev")], repo("dev"))).toBe(1);
  });
});

describe("RepoManagerPanel", () => {
  it("shows a read failure instead of zero skills", () => {
    render(
      <RepoManagerPanel
        repos={[repo("main")]}
        skills={[]}
        failures={[
          {
            owner: "O",
            name: "R",
            branch: "main",
            error: JSON.stringify({ code: "DOWNLOAD_TIMEOUT" }),
          },
        ]}
        onAdd={vi.fn()}
        onRemove={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText("skillsPage.repos.readFailed")).toBeInTheDocument();
    expect(
      screen.queryByText("skills.repo.skillCount"),
    ).not.toBeInTheDocument();
  });
});
