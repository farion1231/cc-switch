import { render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";
import { RepoManagerPanel } from "@/components/skills/RepoManagerPanel";
import type {
  DiscoverableSkill,
  SkillRepo,
  SkillRepoFailure,
} from "@/lib/api/skills";

const repo: SkillRepo = {
  owner: "owner",
  name: "repo",
  branch: "main",
  enabled: true,
};
const skill: DiscoverableSkill = {
  key: "owner/repo:example",
  name: "example",
  description: "",
  directory: "example",
  repoOwner: "owner",
  repoName: "repo",
  repoBranch: "main",
};
const failure: SkillRepoFailure = {
  owner: "owner",
  name: "repo",
  branch: "main",
  error: "timeout",
};

beforeAll(() => {
  i18n.addResourceBundle("zh", "translation", zh, true, true);
});

function show(
  skills: DiscoverableSkill[],
  failures: SkillRepoFailure[] = [],
  isLoading = false,
) {
  render(
    <RepoManagerPanel
      repos={[repo]}
      skills={skills}
      failures={failures}
      isLoading={isLoading}
      onAdd={vi.fn()}
      onRemove={vi.fn()}
      onClose={vi.fn()}
    />,
  );
}

describe("RepoManagerPanel discovery status", () => {
  it("counts main-branch results under an automatically discovered HEAD repository", () => {
    render(
      <RepoManagerPanel
        repos={[{ ...repo, branch: "HEAD" }]}
        skills={[{ ...skill, repoRequestedBranch: "HEAD" }]}
        onAdd={vi.fn()}
        onRemove={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText(/识别到 1 个技能/)).toBeInTheDocument();
  });

  it("shows a pending single-repository scan without turning another repository's genuine zero into loading", () => {
    render(
      <RepoManagerPanel
        repos={[repo, { ...repo, name: "other" }]}
        skills={[]}
        pendingRepos={[repo]}
        onAdd={vi.fn()}
        onRemove={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(
      screen.getByText(new RegExp(i18n.t("skills.loading"))),
    ).toBeInTheDocument();
    expect(screen.getAllByText(/识别到 0 个技能/)).toHaveLength(1);
  });
  it("counts a fallback branch under the configured branch", () => {
    show([{ ...skill, repoBranch: "master", repoRequestedBranch: "main" }]);
    expect(screen.getByText(/识别到 1 个技能/)).toBeInTheDocument();
  });

  it("does not show a scan of an old branch as pending for the current branch", () => {
    render(
      <RepoManagerPanel
        repos={[repo]}
        skills={[]}
        pendingRepos={[{ ...repo, branch: "develop" }]}
        onAdd={vi.fn()}
        onRemove={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText(/识别到 0 个技能/)).toBeInTheDocument();
    expect(
      screen.queryByText(new RegExp(i18n.t("skills.loading"))),
    ).not.toBeInTheDocument();
  });

  it("shows a read failure instead of a zero count when no successful scan exists", () => {
    show([], [failure]);
    expect(screen.getByText(/读取失败/)).toBeInTheDocument();
    expect(screen.queryByText(/识别到 0 个技能/)).not.toBeInTheDocument();
  });

  it("identifies counts from the previous successful scan during a failure", () => {
    show([skill], [failure]);
    expect(screen.getByText(/上次识别到 1 个技能/)).toBeInTheDocument();
    expect(screen.getByText(/读取失败/)).toBeInTheDocument();
  });

  it("does not present an uncompleted initial scan as an empty repository", () => {
    show([], [], true);
    expect(
      screen.getByText(new RegExp(i18n.t("skills.loading"))),
    ).toBeInTheDocument();
    expect(screen.queryByText(/识别到 0 个技能/)).not.toBeInTheDocument();
  });

  it("still shows a genuine zero after a successful empty scan", () => {
    show([]);
    expect(screen.getByText(/识别到 0 个技能/)).toBeInTheDocument();
    expect(screen.queryByText(/读取失败/)).not.toBeInTheDocument();
  });
});
