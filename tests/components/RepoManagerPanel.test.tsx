import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { RepoManagerPanel } from "@/components/skills/RepoManagerPanel";
import type {
  DiscoverableSkill,
  SkillRepo,
  SkillRepoFetchFailure,
} from "@/lib/api/skills";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { count?: number }) =>
      key === "skills.repo.skillCount" ? `skillCount:${options?.count}` : key,
  }),
}));

const repo: SkillRepo = {
  owner: "owner",
  name: "repo",
  branch: "main",
  enabled: true,
};

const skillFromFallbackBranch: DiscoverableSkill = {
  key: "owner/repo:skill",
  name: "Skill",
  description: "",
  directory: "skill",
  repoOwner: "owner",
  repoName: "repo",
  repoBranch: "master",
};

const failureFromFallbackBranch: SkillRepoFetchFailure = {
  owner: "owner",
  name: "repo",
  branch: "master",
  error: "network",
};

const renderPanel = ({
  skills = [],
  failures = [],
}: {
  skills?: DiscoverableSkill[];
  failures?: SkillRepoFetchFailure[];
}) =>
  render(
    <RepoManagerPanel
      repos={[repo]}
      skills={skills}
      failures={failures}
      onRetry={vi.fn()}
      onAdd={vi.fn()}
      onRemove={vi.fn()}
      onClose={vi.fn()}
    />,
  );

describe("RepoManagerPanel", () => {
  it("counts skills resolved from a fallback branch for the configured repository", () => {
    renderPanel({ skills: [skillFromFallbackBranch] });

    expect(screen.getByText("skillCount:1")).toBeInTheDocument();
  });

  it("shows failures resolved from a fallback branch for the configured repository", () => {
    renderPanel({ failures: [failureFromFallbackBranch] });

    expect(screen.getByText("skills.repo.loadFailed")).toBeInTheDocument();
  });
});
