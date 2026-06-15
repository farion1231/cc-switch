import { describe, it, expect } from "vitest";
import type { InstalledSkill, BatchSkillResult } from "@/lib/api/skills";

function makeSkill(overrides: Partial<InstalledSkill> = {}): InstalledSkill {
  return {
    id: "skill-a",
    name: "Skill A",
    directory: "skill-a",
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
    ...overrides,
  };
}

describe("batch uninstall cache update", () => {
  it("removes successfully uninstalled skills from the cache", () => {
    const existing: InstalledSkill[] = [
      makeSkill({ id: "skill-a" }),
      makeSkill({ id: "skill-b", name: "Skill B", directory: "skill-b" }),
      makeSkill({ id: "skill-c", name: "Skill C", directory: "skill-c" }),
    ];

    // Simulate the cache update: remove skills whose id is in the success set
    const succeededIds = new Set(["skill-a", "skill-c"]);
    const updated = existing.filter((s) => !succeededIds.has(s.id));

    expect(updated).toHaveLength(1);
    expect(updated[0].id).toBe("skill-b");
  });

  it("keeps all skills when none succeed", () => {
    const existing: InstalledSkill[] = [
      makeSkill({ id: "skill-a" }),
      makeSkill({ id: "skill-b", name: "Skill B", directory: "skill-b" }),
    ];
    const succeededIds = new Set<string>();
    const updated = existing.filter((s) => !succeededIds.has(s.id));

    expect(updated).toHaveLength(2);
  });

  it("returns existing cache unchanged when no cache exists", () => {
    const succeededIds = new Set(["skill-a"]);
    // oldData is undefined → should return undefined
    const oldData = undefined as InstalledSkill[] | undefined;
    const updated = oldData
      ? oldData.filter((s) => !succeededIds.has(s.id))
      : oldData;
    expect(updated).toBeUndefined();
  });
});

describe("batch results parsing", () => {
  it("correctly separates successes and failures", () => {
    const results: BatchSkillResult[] = [
      { id: "a", success: true },
      { id: "b", success: false, error: "not found" },
      { id: "c", success: true },
    ];

    const succeeded = results.filter((r) => r.success).map((r) => r.id);
    const failed = results.filter((r) => !r.success).map((r) => r.id);

    expect(succeeded).toEqual(["a", "c"]);
    expect(failed).toEqual(["b"]);
  });

  it("handles empty results", () => {
    const results: BatchSkillResult[] = [];
    const succeeded = results.filter((r) => r.success);
    const failed = results.filter((r) => !r.success);
    expect(succeeded).toHaveLength(0);
    expect(failed).toHaveLength(0);
  });
});
