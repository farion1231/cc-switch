import { beforeEach, describe, expect, it } from "vitest";

import {
  applySkillDiscoveryProgress,
  beginSkillDiscovery,
  failSkillDiscovery,
  finishSkillDiscovery,
  getSkillDiscoveryTaskSnapshot,
  removeSkillDiscoveryRepository,
  resetSkillDiscoveryTask,
  setSkillDiscoveryRepositoryResult,
} from "@/stores/skillDiscoveryTask";

describe("skillDiscoveryTask", () => {
  beforeEach(() => {
    resetSkillDiscoveryTask();
  });

  it("keeps an in-flight discovery active independently of page mounts", () => {
    beginSkillDiscovery();
    applySkillDiscoveryProgress({
      phase: "loading",
      completed: 1,
      total: 5,
      repo: "anthropics/skills",
    });

    expect(getSkillDiscoveryTaskSnapshot()).toMatchObject({
      active: true,
      completed: 1,
      total: 5,
    });
    expect(
      getSkillDiscoveryTaskSnapshot().repositories["anthropics/skills"],
    ).toMatchObject({ phase: "loading" });
  });

  it("starts a distinct task when a second network refresh supersedes the first", () => {
    const firstTask = beginSkillDiscovery();
    applySkillDiscoveryProgress({
      phase: "loading",
      completed: 1,
      total: 5,
      repo: "stale/repository",
      requestId: firstTask,
    });

    const secondTask = beginSkillDiscovery(true);
    applySkillDiscoveryProgress({
      phase: "completed",
      completed: 5,
      total: 5,
      repo: "stale/repository",
      requestId: firstTask,
    });

    expect(secondTask).not.toBe(firstTask);
    expect(getSkillDiscoveryTaskSnapshot()).toMatchObject({
      taskId: secondTask,
      active: true,
      completed: 0,
      total: 0,
      repositories: {},
    });
  });

  it("finishes from the command result even if terminal progress events were missed", () => {
    beginSkillDiscovery();
    finishSkillDiscovery({
      skills: [
        {
          key: "anthropics/skills:frontend-design",
          name: "Frontend Design",
          description: "",
          directory: "frontend-design",
          repoOwner: "anthropics",
          repoName: "skills",
          repoBranch: "main",
        },
      ],
      failures: [
        {
          owner: "JimLiu",
          name: "baoyu-skills",
          branch: "main",
          error: "timeout",
        },
      ],
    });

    expect(getSkillDiscoveryTaskSnapshot()).toMatchObject({
      active: false,
      completed: 2,
      total: 2,
    });
    expect(
      getSkillDiscoveryTaskSnapshot().repositories["anthropics/skills"],
    ).toMatchObject({ phase: "completed", skillCount: 1 });
    expect(
      getSkillDiscoveryTaskSnapshot().repositories["JimLiu/baoyu-skills"],
    ).toMatchObject({ phase: "failed" });
  });

  it("does not reactivate the task when the final event arrives after the command result", () => {
    beginSkillDiscovery();
    finishSkillDiscovery({
      skills: [],
      failures: [
        {
          owner: "JimLiu",
          name: "baoyu-skills",
          branch: "main",
          error: "timeout",
        },
      ],
    });

    applySkillDiscoveryProgress({
      phase: "failed",
      completed: 1,
      total: 1,
      repo: "JimLiu/baoyu-skills",
      error: "timeout",
    });

    expect(getSkillDiscoveryTaskSnapshot().active).toBe(false);
  });

  it("does not leave repository spinners running after the discovery command fails", () => {
    beginSkillDiscovery();
    applySkillDiscoveryProgress({
      phase: "loading",
      completed: 0,
      total: 1,
      repo: "anthropics/skills",
    });

    failSkillDiscovery();

    expect(getSkillDiscoveryTaskSnapshot()).toMatchObject({
      active: false,
      repositories: {
        "anthropics/skills": {
          phase: "failed",
        },
      },
    });
  });

  it("removes a deleted repository and ignores its remaining progress", () => {
    beginSkillDiscovery();
    applySkillDiscoveryProgress({
      phase: "scanning",
      completed: 0,
      total: 2,
      repo: "removed/repo",
    });

    removeSkillDiscoveryRepository("removed/repo");
    applySkillDiscoveryProgress({
      phase: "completed",
      completed: 1,
      total: 2,
      repo: "removed/repo",
      skillCount: 1,
      skills: [
        {
          key: "removed/repo:stale",
          name: "Stale",
          description: "",
          directory: "stale",
          repoOwner: "removed",
          repoName: "repo",
          repoBranch: "main",
        },
      ],
    });

    expect(
      getSkillDiscoveryTaskSnapshot().repositories["removed/repo"],
    ).toBeUndefined();
    expect(getSkillDiscoveryTaskSnapshot()).toMatchObject({
      active: true,
      completed: 0,
      total: 1,
    });
  });

  it("allows a re-added repository retry to replace its deleted or failed status", () => {
    beginSkillDiscovery();
    removeSkillDiscoveryRepository("owner/repo");

    setSkillDiscoveryRepositoryResult("owner/repo", {
      skills: [
        {
          key: "owner/repo:skill",
          name: "Skill",
          description: "",
          directory: "skill",
          repoOwner: "owner",
          repoName: "repo",
          repoBranch: "main",
        },
      ],
      failures: [],
    });

    expect(
      getSkillDiscoveryTaskSnapshot().repositories["owner/repo"],
    ).toMatchObject({
      phase: "completed",
      skillCount: 1,
    });
  });
});
