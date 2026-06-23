import { describe, expect, it } from "vitest";

import { filterUpdateCheckResultForInstalledSkills } from "@/hooks/useSkills";
import type { InstalledSkill, SkillUpdateCheckResult } from "@/lib/api/skills";

const installed = (
  overrides: Partial<InstalledSkill> = {},
): InstalledSkill => ({
  id: "skill-1",
  name: "Skill One",
  directory: "skill-one",
  repoOwner: "Owner",
  repoName: "Repo",
  repoBranch: "main",
  apps: {
    claude: true,
    codex: false,
    gemini: false,
    opencode: false,
    openclaw: false,
    hermes: false,
  },
  installedAt: 1,
  updatedAt: 1,
  ...overrides,
});

const checkResult: SkillUpdateCheckResult = {
  updates: [
    { id: "skill-1", name: "Skill One", remoteHash: "remote-1" },
    { id: "removed-skill", name: "Removed Skill", remoteHash: "remote-2" },
  ],
  failures: [
    { owner: "owner", name: "repo", branch: "main", error: "timeout" },
    {
      owner: "removed-owner",
      name: "removed-repo",
      branch: "main",
      error: "timeout",
    },
  ],
};

describe("filterUpdateCheckResultForInstalledSkills", () => {
  it("removes updates and failures for skills that are no longer installed", () => {
    const filtered = filterUpdateCheckResultForInstalledSkills(checkResult, [
      installed(),
    ]);

    expect(filtered.updates.map((update) => update.id)).toEqual(["skill-1"]);
    expect(filtered.failures.map((failure) => failure.owner)).toEqual([
      "owner",
    ]);
  });

  it("removes stale updates when the same skill was replaced during the check", () => {
    const filtered = filterUpdateCheckResultForInstalledSkills(
      { updates: [checkResult.updates[0]], failures: [] },
      [installed({ installedAt: 2, contentHash: "new-local" })],
      [installed({ installedAt: 1, contentHash: "old-local" })],
    );

    expect(filtered.updates).toEqual([]);
  });

  it("removes skill-specific failures when only another skill from the same repo remains", () => {
    const filtered = filterUpdateCheckResultForInstalledSkills(
      {
        updates: [],
        failures: [
          {
            owner: "owner",
            name: "repo",
            branch: "main",
            skillId: "removed-skill",
            error: "SKILL_DIR_NOT_FOUND: removed-skill",
          },
        ],
      },
      [installed({ id: "other-skill", directory: "other-skill" })],
      [installed({ id: "removed-skill", directory: "removed-skill" })],
    );

    expect(filtered.failures).toEqual([]);
  });
});
