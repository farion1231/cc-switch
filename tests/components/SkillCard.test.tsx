import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { SkillCard } from "@/components/skills/SkillCard";

const makeSkill = (compatibility?: string) => ({
  key: "owner/repo:browser-check",
  name: "browser-check",
  description: "Checks web pages",
  compatibility,
  directory: "browser-check",
  readmeUrl: "https://github.com/owner/repo/blob/main/SKILL.md",
  repoOwner: "owner",
  repoName: "repo",
  repoBranch: "main",
  installed: false,
});

describe("SkillCard", () => {
  it("shows standard compatibility requirements before installation", () => {
    render(
      <SkillCard
        skill={makeSkill("Requires agent-browser and network access")}
        onInstall={vi.fn()}
        onUninstall={vi.fn()}
      />,
    );

    expect(screen.getByText("skills.requirements:")).toBeInTheDocument();
    expect(
      screen.getByText("Requires agent-browser and network access"),
    ).toBeInTheDocument();
  });

  it("does not render an empty requirements section", () => {
    render(
      <SkillCard
        skill={makeSkill()}
        onInstall={vi.fn()}
        onUninstall={vi.fn()}
      />,
    );

    expect(screen.queryByText("skills.requirements")).not.toBeInTheDocument();
  });
});
