import { describe, it, expect } from "vitest";
import { mergeImportedSkills } from "@/hooks/useSkills.helpers";
import type { InstalledSkill } from "@/lib/api/skills";

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

// Regression coverage for issue #2139: when a user double-clicks the import
// button (or the mutation otherwise fires twice with the same payload), the
// installed cache must not accumulate duplicate entries for the same skill.
describe("mergeImportedSkills", () => {
  it("returns the imported list as-is when no cache exists yet", () => {
    const imported = [makeSkill()];
    expect(mergeImportedSkills(undefined, imported)).toEqual(imported);
  });

  it("dedupes by id when the same skill is imported twice in a row", () => {
    const existing = [makeSkill()];
    const secondImport = [makeSkill()];
    const merged = mergeImportedSkills(existing, secondImport);
    expect(merged).toHaveLength(1);
    expect(merged[0]).toBe(secondImport[0]);
  });

  it("replaces stale cache entries with fresh imports for the same id", () => {
    const stale = [makeSkill({ name: "Stale Name" })];
    const fresh = [makeSkill({ name: "Fresh Name" })];
    const merged = mergeImportedSkills(stale, fresh);
    expect(merged).toHaveLength(1);
    expect(merged[0].name).toBe("Fresh Name");
  });

  it("returns the existing reference unchanged when the imported list is empty", () => {
    const existing = [makeSkill()];
    expect(mergeImportedSkills(existing, [])).toBe(existing);
  });

  it("appends newly imported skills without dropping existing unrelated ones", () => {
    const existing = [makeSkill({ id: "skill-a", directory: "skill-a" })];
    const imported = [
      makeSkill({ id: "skill-b", directory: "skill-b", name: "Skill B" }),
    ];
    const merged = mergeImportedSkills(existing, imported);
    expect(merged.map((s) => s.id).sort()).toEqual(["skill-a", "skill-b"]);
  });
});
