import { describe, expect, it } from "vitest";

import {
  filterSkillDiscoveryResultForRepositories,
  filterSkillUpdatesForInstalled,
  mergeSkillDiscoveryResult,
} from "@/hooks/useSkills.helpers";
import type {
  InstalledSkill,
  SkillDiscoveryResult,
  SkillRepo,
  SkillUpdateCheckResult,
} from "@/lib/api/skills";

const previous: SkillDiscoveryResult = {
  skills: [
    {
      key: "anthropics/skills:frontend-design",
      name: "Frontend Design",
      description: "Previous cached skill",
      directory: "frontend-design",
      repoOwner: "anthropics",
      repoName: "skills",
      repoBranch: "main",
    },
    {
      key: "obra/superpowers:brainstorming",
      name: "Brainstorming",
      description: "Old version",
      directory: "brainstorming",
      repoOwner: "obra",
      repoName: "superpowers",
      repoBranch: "main",
    },
  ],
  failures: [],
};

describe("mergeSkillDiscoveryResult", () => {
  it("keeps the last successful skills for repositories that failed to refresh", () => {
    const next: SkillDiscoveryResult = {
      skills: [
        {
          key: "obra/superpowers:brainstorming",
          name: "Brainstorming Updated",
          description: "Fresh version",
          directory: "brainstorming",
          repoOwner: "obra",
          repoName: "superpowers",
          repoBranch: "main",
        },
      ],
      failures: [
        {
          owner: "anthropics",
          name: "skills",
          branch: "main",
          error: "timeout",
        },
      ],
    };

    const merged = mergeSkillDiscoveryResult(previous, next);

    expect(merged.skills).toEqual([
      expect.objectContaining({
        repoOwner: "anthropics",
        name: "Frontend Design",
      }),
      expect.objectContaining({
        repoOwner: "obra",
        name: "Brainstorming Updated",
      }),
    ]);
    expect(merged.failures).toEqual(next.failures);
  });

  it("does not replace newer in-memory skills with stale fallback data after a failed refresh", () => {
    const newerInMemory: SkillDiscoveryResult = {
      skills: [
        {
          key: "anthropics/skills:frontend-design",
          name: "Frontend Design",
          description: "Newer in-memory result",
          directory: "frontend-design",
          repoOwner: "anthropics",
          repoName: "skills",
          repoBranch: "main",
        },
      ],
      failures: [],
    };
    const failedWithStaleFallback: SkillDiscoveryResult = {
      skills: [
        {
          ...newerInMemory.skills[0],
          description: "Older persisted fallback",
        },
      ],
      failures: [
        {
          owner: "anthropics",
          name: "skills",
          branch: "main",
          error: "network unavailable",
        },
      ],
    };

    const merged = mergeSkillDiscoveryResult(
      newerInMemory,
      failedWithStaleFallback,
    );

    expect(merged.skills).toEqual([
      expect.objectContaining({ description: "Newer in-memory result" }),
    ]);
    expect(merged.failures).toEqual(failedWithStaleFallback.failures);
  });

  it("does not resurrect stale skills after the latest successful refresh was empty", () => {
    const latestEmptyResult: SkillDiscoveryResult = {
      skills: [],
      failures: [],
      refreshedRepositories: [
        {
          owner: "anthropics",
          name: "skills",
          branch: "main",
        },
      ],
    };
    const failedWithStaleFallback: SkillDiscoveryResult = {
      skills: [
        {
          ...previous.skills[0],
          description: "Stale persisted fallback",
        },
      ],
      failures: [
        {
          owner: "anthropics",
          name: "skills",
          branch: "main",
          error: "network unavailable",
        },
      ],
    };

    const merged = mergeSkillDiscoveryResult(
      latestEmptyResult,
      failedWithStaleFallback,
    );

    expect(merged.skills).toEqual([]);
    expect(merged.refreshedRepositories).toEqual(
      latestEmptyResult.refreshedRepositories,
    );
  });

  it("does not retain old skills after a repository refresh succeeds", () => {
    const next: SkillDiscoveryResult = {
      skills: [
        {
          key: "anthropics/skills:new-skill",
          name: "New Skill",
          description: "",
          directory: "new-skill",
          repoOwner: "anthropics",
          repoName: "skills",
          repoBranch: "main",
        },
      ],
      failures: [],
    };

    const merged = mergeSkillDiscoveryResult(previous, next);

    expect(
      merged.skills.some((skill) => skill.name === "Frontend Design"),
    ).toBe(false);
    expect(merged.skills.some((skill) => skill.name === "New Skill")).toBe(
      true,
    );
  });

  it("removes old skills when a repository successfully refreshes to an empty result", () => {
    const next = {
      skills: [],
      failures: [
        {
          owner: "anthropics",
          name: "skills",
          branch: "main",
          error: "no skills found",
        },
      ],
      refreshedRepositories: [
        {
          owner: "anthropics",
          name: "skills",
          branch: "main",
        },
      ],
    } as SkillDiscoveryResult;

    const merged = mergeSkillDiscoveryResult(previous, next);

    expect(
      merged.skills.some((skill) => skill.repoOwner === "anthropics"),
    ).toBe(false);
    expect(merged.skills.some((skill) => skill.repoOwner === "obra")).toBe(
      true,
    );
    expect(merged.failures).toEqual(next.failures);
  });

  it("replaces stale skills when a repository resolves to a different branch", () => {
    const previousFromFallback: SkillDiscoveryResult = {
      skills: [
        {
          key: "owner/repo:removed-skill",
          name: "Removed Skill",
          description: "",
          directory: "removed-skill",
          repoOwner: "owner",
          repoName: "repo",
          repoBranch: "master",
        },
      ],
      failures: [],
    };
    const nextFromConfiguredBranch: SkillDiscoveryResult = {
      skills: [
        {
          key: "owner/repo:current-skill",
          name: "Current Skill",
          description: "",
          directory: "current-skill",
          repoOwner: "owner",
          repoName: "repo",
          repoBranch: "main",
        },
      ],
      failures: [],
    };

    const merged = mergeSkillDiscoveryResult(
      previousFromFallback,
      nextFromConfiguredBranch,
    );

    expect(merged.skills.map((skill) => skill.name)).toEqual(["Current Skill"]);
  });

  it("keeps skills from a repository added after an older refresh started", () => {
    const next: SkillDiscoveryResult = {
      skills: [
        {
          key: "anthropics/skills:new-skill",
          name: "New Skill",
          description: "",
          directory: "new-skill",
          repoOwner: "anthropics",
          repoName: "skills",
          repoBranch: "main",
        },
      ],
      failures: [],
    };
    const previousWithNewRepository: SkillDiscoveryResult = {
      ...previous,
      skills: [
        ...previous.skills,
        {
          key: "new/repo:new-skill",
          name: "New Repository Skill",
          description: "",
          directory: "new-skill",
          repoOwner: "new",
          repoName: "repo",
          repoBranch: "main",
        },
      ],
    };

    const merged = mergeSkillDiscoveryResult(previousWithNewRepository, next);

    expect(merged.skills.some((skill) => skill.repoOwner === "new")).toBe(true);
  });

  it("drops late results for a repository removed while discovery was running", () => {
    const configured: SkillRepo[] = [
      {
        owner: "obra",
        name: "superpowers",
        branch: "main",
        enabled: true,
      },
    ];

    const filtered = filterSkillDiscoveryResultForRepositories(
      previous,
      configured,
    );

    expect(filtered.skills).toEqual([
      expect.objectContaining({ repoOwner: "obra" }),
    ]);
  });

  it("keeps results from the actual fallback branch of a configured repository", () => {
    const configured: SkillRepo[] = [
      {
        owner: "obra",
        name: "superpowers",
        branch: "main",
        enabled: true,
      },
    ];
    const fallbackResult: SkillDiscoveryResult = {
      skills: [
        {
          ...previous.skills[1],
          repoBranch: "master",
        },
      ],
      failures: [],
    };

    expect(
      filterSkillDiscoveryResultForRepositories(fallbackResult, configured)
        .skills,
    ).toHaveLength(1);
  });
});

describe("filterSkillUpdatesForInstalled", () => {
  it("drops update results for a skill uninstalled while the check was running", () => {
    const installed: InstalledSkill[] = [
      {
        id: "still-installed",
        name: "Still installed",
        directory: "still-installed",
        apps: {
          claude: true,
          codex: false,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        },
        installedAt: 0,
        updatedAt: 0,
      },
    ];
    const result: SkillUpdateCheckResult = {
      updates: [
        {
          id: "still-installed",
          name: "Still installed",
          remoteHash: "remote-a",
          status: "updateAvailable",
        },
        {
          id: "already-removed",
          name: "Already removed",
          remoteHash: "remote-b",
          status: "updateAvailable",
        },
      ],
      failures: [],
    };

    expect(filterSkillUpdatesForInstalled(result, installed).updates).toEqual([
      result.updates[0],
    ]);
  });

  it("drops repository failures that no longer belong to installed skills", () => {
    const installed: InstalledSkill[] = [
      {
        id: "anthropics/skills:still-installed",
        name: "Still installed",
        directory: "still-installed",
        repoOwner: "anthropics",
        repoName: "skills",
        repoBranch: "main",
        apps: {
          claude: true,
          codex: false,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        },
        installedAt: 0,
        updatedAt: 0,
      },
    ];
    const result: SkillUpdateCheckResult = {
      updates: [],
      failures: [
        {
          owner: "anthropics",
          name: "skills",
          branch: "main",
          error: "network",
        },
        {
          owner: "removed",
          name: "repo",
          branch: "main",
          error: "network",
        },
      ],
    };

    expect(filterSkillUpdatesForInstalled(result, installed).failures).toEqual([
      result.failures[0],
    ]);
  });

  it("drops a result when the same skill was replaced while the check was running", () => {
    const before: InstalledSkill[] = [
      {
        id: "restored-skill",
        name: "Restored skill",
        directory: "restored-skill",
        apps: {
          claude: true,
          codex: false,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        },
        installedAt: 1,
        contentHash: "before",
        updatedAt: 0,
      },
    ];
    const current = [
      {
        ...before[0],
        installedAt: 2,
        contentHash: "restored",
      },
    ];
    const result: SkillUpdateCheckResult = {
      updates: [
        {
          id: "restored-skill",
          name: "Restored skill",
          remoteHash: "remote",
          status: "updateAvailable",
        },
      ],
      failures: [],
    };

    expect(
      filterSkillUpdatesForInstalled(result, current, before).updates,
    ).toEqual([]);
  });
});
