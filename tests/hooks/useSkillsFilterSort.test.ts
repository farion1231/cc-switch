import { describe, it, expect } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { InstalledSkill, SkillUpdateInfo } from "@/lib/api/skills";
import type { AppId } from "@/lib/api/types";
import {
  ALL_GROUP_KEY,
  computeGroups,
  deriveSourceOptions,
  filterSkills,
  getSourceKey,
  groupSkills,
  LOCAL_SOURCE_KEY,
  matchSkillBySearch,
  sortSkills,
  UNASSIGNED_GROUP_KEY,
  useSkillsFilterSort,
  type FilterSortState,
} from "@/components/skills/useSkillsFilterSort";

const APP_IDS: AppId[] = [
  "claude",
  "codex",
  "gemini",
  "opencode",
  "hermes",
];

function makeSkill(overrides: Partial<InstalledSkill> = {}): InstalledSkill {
  return {
    id: "skill-a",
    name: "Skill A",
    directory: "skill-a",
    repoOwner: "forrest",
    repoName: "kit",
    apps: {
      claude: true,
      codex: false,
      gemini: false,
      opencode: false,
      openclaw: false,
      hermes: false,
    },
    installedAt: 1000,
    updatedAt: 0,
    ...overrides,
  };
}

function emptyState(
  overrides: Partial<FilterSortState> = {},
): Pick<
  FilterSortState,
  | "searchQuery"
  | "filterSources"
  | "filterApps"
  | "filterUpdateOnly"
  | "sortKey"
  | "groupKey"
> {
  return {
    searchQuery: overrides.searchQuery ?? "",
    filterSources: overrides.filterSources ?? new Set(),
    filterApps: overrides.filterApps ?? new Set(),
    filterUpdateOnly: overrides.filterUpdateOnly ?? false,
    sortKey: overrides.sortKey ?? "nameAsc",
    groupKey: overrides.groupKey ?? "none",
  };
}

describe("getSourceKey", () => {
  it("returns owner/name for repo-backed skill", () => {
    expect(getSourceKey(makeSkill())).toBe("forrest/kit");
  });

  it("returns LOCAL_SOURCE_KEY when repo info missing", () => {
    const s = makeSkill({ repoOwner: undefined, repoName: undefined });
    expect(getSourceKey(s)).toBe(LOCAL_SOURCE_KEY);
  });
});

describe("matchSkillBySearch", () => {
  const skill = makeSkill({
    id: "x",
    name: "Auth Flow Helper",
    description: "OAuth login utility",
    directory: "auth-helper",
    repoOwner: "forrest",
    repoName: "kit",
  });

  it("returns true for empty query", () => {
    expect(matchSkillBySearch(skill, "")).toBe(true);
  });

  it("matches by name (case-insensitive)", () => {
    expect(matchSkillBySearch(skill, "auth")).toBe(true);
    expect(matchSkillBySearch(skill, "AUTH")).toBe(true);
  });

  it("matches by description", () => {
    expect(matchSkillBySearch(skill, "oauth login")).toBe(true);
  });

  it("matches by directory", () => {
    expect(matchSkillBySearch(skill, "helper")).toBe(true);
  });

  it("matches by owner or name", () => {
    expect(matchSkillBySearch(skill, "forrest")).toBe(true);
    expect(matchSkillBySearch(skill, "kit")).toBe(true);
  });

  it("matches by composite owner/name", () => {
    expect(matchSkillBySearch(skill, "forrest/kit")).toBe(true);
  });

  it("returns false when nothing matches", () => {
    expect(matchSkillBySearch(skill, "nope-zzz")).toBe(false);
  });
});

describe("filterSkills", () => {
  const skills: InstalledSkill[] = [
    makeSkill({ id: "a", repoOwner: "forrest", repoName: "kit", apps: appsWith(["claude"]) }),
    makeSkill({ id: "b", repoOwner: "kar", repoName: "tools", apps: appsWith(["codex"]) }),
    makeSkill({ id: "c", repoOwner: undefined, repoName: undefined, apps: appsWith(["claude", "gemini"]) }),
  ];

  it("returns all when no filters", () => {
    expect(filterSkills(skills, emptyState(), {})).toHaveLength(3);
  });

  it("filterSources is union (OR within axis)", () => {
    const out = filterSkills(
      skills,
      emptyState({ filterSources: new Set(["forrest/kit", "kar/tools"]) }),
      {},
    );
    expect(out.map((s) => s.id)).toEqual(["a", "b"]);
  });

  it("filterApps is OR within axis", () => {
    const out = filterSkills(
      skills,
      emptyState({ filterApps: new Set<AppId>(["codex", "gemini"]) }),
      {},
    );
    expect(out.map((s) => s.id).sort()).toEqual(["b", "c"]);
  });

  it("crosses axes with AND", () => {
    const out = filterSkills(
      skills,
      emptyState({
        filterSources: new Set(["forrest/kit"]),
        filterApps: new Set<AppId>(["claude"]),
      }),
      {},
    );
    expect(out.map((s) => s.id)).toEqual(["a"]);
  });

  it("filterUpdateOnly leaves only skills present in updatesMap", () => {
    const updatesMap: Record<string, SkillUpdateInfo> = {
      a: { id: "a", name: "Skill A", remoteHash: "abc" },
    };
    const out = filterSkills(
      skills,
      emptyState({ filterUpdateOnly: true }),
      updatesMap,
    );
    expect(out.map((s) => s.id)).toEqual(["a"]);
  });

  it("local source key matches no-repo skill", () => {
    const out = filterSkills(
      skills,
      emptyState({ filterSources: new Set([LOCAL_SOURCE_KEY]) }),
      {},
    );
    expect(out.map((s) => s.id)).toEqual(["c"]);
  });
});

describe("sortSkills", () => {
  const skills = [
    makeSkill({ id: "z", name: "Zebra", installedAt: 100, repoOwner: "z", repoName: "z" }),
    makeSkill({ id: "a", name: "Apple", installedAt: 300, repoOwner: "a", repoName: "a" }),
    makeSkill({ id: "m", name: "Mango", installedAt: 200, repoOwner: "m", repoName: "m" }),
  ];

  it("sorts by name ascending", () => {
    expect(sortSkills(skills, "nameAsc").map((s) => s.id)).toEqual(["a", "m", "z"]);
  });

  it("sorts by name descending", () => {
    expect(sortSkills(skills, "nameDesc").map((s) => s.id)).toEqual(["z", "m", "a"]);
  });

  it("sorts by installedAt newest first", () => {
    expect(sortSkills(skills, "installedNewest").map((s) => s.id)).toEqual(["a", "m", "z"]);
  });

  it("sorts by installedAt oldest first", () => {
    expect(sortSkills(skills, "installedOldest").map((s) => s.id)).toEqual(["z", "m", "a"]);
  });

  it("sorts by source key ascending", () => {
    expect(sortSkills(skills, "sourceAsc").map((s) => s.id)).toEqual(["a", "m", "z"]);
  });

  it("places pinned skills before non-pinned regardless of sort key", () => {
    const withPin = [
      ...skills,
      makeSkill({ id: "p", name: "Zebra-Pinned", installedAt: 50, pinnedAt: 9999 }),
    ];
    const out = sortSkills(withPin, "nameAsc");
    expect(out[0].id).toBe("p");
  });

  it("orders multiple pinned skills by pinnedAt descending", () => {
    const pins = [
      makeSkill({ id: "p1", name: "A", pinnedAt: 100 }),
      makeSkill({ id: "p2", name: "Z", pinnedAt: 200 }),
    ];
    expect(sortSkills(pins, "nameAsc").map((s) => s.id)).toEqual(["p2", "p1"]);
  });
});

describe("groupSkills", () => {
  const skills = [
    makeSkill({ id: "a", repoOwner: "forrest", repoName: "kit", apps: appsWith(["claude"]) }),
    makeSkill({ id: "b", repoOwner: "kar", repoName: "tools", apps: appsWith(["codex", "claude"]) }),
    makeSkill({ id: "c", repoOwner: undefined, repoName: undefined, apps: appsWith(["gemini"]) }),
  ];

  it("none returns a single all-group", () => {
    const out = groupSkills(skills, "none", APP_IDS);
    expect(out).toHaveLength(1);
    expect(out[0].key).toBe(ALL_GROUP_KEY);
    expect(out[0].items).toHaveLength(3);
  });

  it("source groups have one entry per unique source; local sorted last", () => {
    const out = groupSkills(skills, "source", APP_IDS);
    expect(out.map((g) => g.key)).toEqual([
      "forrest/kit",
      "kar/tools",
      LOCAL_SOURCE_KEY,
    ]);
    expect(out.reduce((n, g) => n + g.items.length, 0)).toBe(3);
  });

  it("app groups one per app id; skill repeats across enabled apps", () => {
    const out = groupSkills(skills, "app", APP_IDS);
    expect(out.map((g) => g.key)).toEqual(APP_IDS);
    const totalAcross = out.reduce((n, g) => n + g.items.length, 0);
    // a→claude(1), b→codex+claude(2), c→gemini(1) = 4 occurrences
    expect(totalAcross).toBe(4);
    const claudeGroup = out.find((g) => g.key === "claude")!;
    expect(claudeGroup.items.map((s) => s.id).sort()).toEqual(["a", "b"]);
  });

  it("app grouping surfaces skills with no enabled app in an unassigned group", () => {
    const orphan = makeSkill({
      id: "orphan",
      name: "Orphan",
      apps: appsWith([]),
    });
    const out = groupSkills([...skills, orphan], "app", APP_IDS);
    const unassigned = out.find((g) => g.key === UNASSIGNED_GROUP_KEY);
    expect(unassigned).toBeDefined();
    expect(unassigned!.items.map((s) => s.id)).toEqual(["orphan"]);
    // The unassigned group only appears when needed
    const outNoOrphans = groupSkills(skills, "app", APP_IDS);
    expect(
      outNoOrphans.find((g) => g.key === UNASSIGNED_GROUP_KEY),
    ).toBeUndefined();
  });
});

describe("deriveSourceOptions", () => {
  it("returns sorted unique sources with local key last", () => {
    const skills = [
      makeSkill({ id: "a", repoOwner: "z", repoName: "z" }),
      makeSkill({ id: "b", repoOwner: "a", repoName: "x" }),
      makeSkill({ id: "c", repoOwner: undefined, repoName: undefined }),
      makeSkill({ id: "d", repoOwner: "a", repoName: "x" }),
    ];
    expect(deriveSourceOptions(skills)).toEqual([
      "a/x",
      "z/z",
      LOCAL_SOURCE_KEY,
    ]);
  });
});

describe("computeGroups (pipeline)", () => {
  it("empty input yields empty groups (none mode)", () => {
    const out = computeGroups([], emptyState(), {}, APP_IDS);
    expect(out.filtered).toHaveLength(0);
    expect(out.groups[0].items).toHaveLength(0);
  });

  it("end-to-end: search + source filter + sort + group", () => {
    const skills = [
      makeSkill({ id: "a", name: "Apple", repoOwner: "x", repoName: "y", installedAt: 100 }),
      makeSkill({ id: "b", name: "Banana", repoOwner: "x", repoName: "y", installedAt: 200 }),
      makeSkill({ id: "c", name: "Apricot", repoOwner: "p", repoName: "q", installedAt: 300 }),
    ];
    const out = computeGroups(
      skills,
      emptyState({
        searchQuery: "ap",
        filterSources: new Set(["x/y"]),
        sortKey: "installedNewest",
        groupKey: "source",
      }),
      {},
      APP_IDS,
    );
    // search "ap" → Apple, Apricot
    // source filter "x/y" → only Apple
    // group by source → only "x/y"
    expect(out.filtered.map((s) => s.id)).toEqual(["a"]);
    expect(out.groups).toHaveLength(1);
    expect(out.groups[0].key).toBe("x/y");
  });

  it("pinned floats to top inside its group", () => {
    const skills = [
      makeSkill({ id: "a", name: "Apple", repoOwner: "x", repoName: "y" }),
      makeSkill({ id: "b", name: "Banana", repoOwner: "x", repoName: "y", pinnedAt: 100 }),
    ];
    const out = computeGroups(
      skills,
      emptyState({ groupKey: "source", sortKey: "nameAsc" }),
      {},
      APP_IDS,
    );
    expect(out.groups[0].items.map((s) => s.id)).toEqual(["b", "a"]);
  });
});

describe("useSkillsFilterSort (hook)", () => {
  const skills = [
    makeSkill({ id: "a", name: "Apple", repoOwner: "x", repoName: "y" }),
    makeSkill({ id: "b", name: "Banana", repoOwner: "x", repoName: "y" }),
    makeSkill({
      id: "c",
      name: "Cherry",
      repoOwner: undefined,
      repoName: undefined,
    }),
  ];

  it("initial state: no filters, defaults applied", () => {
    const { result } = renderHook(() =>
      useSkillsFilterSort(skills, {}, APP_IDS),
    );
    expect(result.current.total).toBe(3);
    expect(result.current.filteredCount).toBe(3);
    expect(result.current.hasFilters).toBe(false);
    expect(result.current.state.sortKey).toBe("nameAsc");
    expect(result.current.state.groupKey).toBe("none");
    expect(result.current.state.selectionMode).toBe(false);
  });

  it("setSearchQuery sets searchQuery and impacts hasFilters", () => {
    const { result } = renderHook(() =>
      useSkillsFilterSort(skills, {}, APP_IDS),
    );
    act(() => result.current.setSearchQuery("apple"));
    expect(result.current.state.searchQuery).toBe("apple");
    expect(result.current.hasFilters).toBe(true);
    expect(result.current.filteredCount).toBe(1);
  });

  it("toggleSource adds and removes a source filter", () => {
    const { result } = renderHook(() =>
      useSkillsFilterSort(skills, {}, APP_IDS),
    );
    act(() => result.current.toggleSource("x/y"));
    expect(result.current.state.filterSources.has("x/y")).toBe(true);
    expect(result.current.filteredCount).toBe(2);

    act(() => result.current.toggleSource("x/y"));
    expect(result.current.state.filterSources.has("x/y")).toBe(false);
    expect(result.current.filteredCount).toBe(3);
  });

  it("toggleApp adds and removes app filters", () => {
    const { result } = renderHook(() =>
      useSkillsFilterSort(skills, {}, APP_IDS),
    );
    act(() => result.current.toggleApp("claude"));
    expect(result.current.state.filterApps.has("claude")).toBe(true);
    act(() => result.current.toggleApp("claude"));
    expect(result.current.state.filterApps.has("claude")).toBe(false);
  });

  it("setFilterUpdateOnly and clearFilters reset state", () => {
    const { result } = renderHook(() =>
      useSkillsFilterSort(skills, {}, APP_IDS),
    );
    act(() => {
      result.current.setSearchQuery("apple");
      result.current.toggleSource("x/y");
      result.current.setFilterUpdateOnly(true);
    });
    expect(result.current.hasFilters).toBe(true);

    act(() => result.current.clearFilters());
    expect(result.current.hasFilters).toBe(false);
    expect(result.current.state.searchQuery).toBe("");
    expect(result.current.state.filterSources.size).toBe(0);
    expect(result.current.state.filterUpdateOnly).toBe(false);
  });

  it("toggleCollapsed toggles a group key in collapsed set", () => {
    const { result } = renderHook(() =>
      useSkillsFilterSort(skills, {}, APP_IDS),
    );
    act(() => result.current.toggleCollapsed("x/y"));
    expect(result.current.state.collapsed.has("x/y")).toBe(true);
    act(() => result.current.toggleCollapsed("x/y"));
    expect(result.current.state.collapsed.has("x/y")).toBe(false);
  });

  it("selection mode lifecycle: enter, toggle, select all, exit", () => {
    const { result } = renderHook(() =>
      useSkillsFilterSort(skills, {}, APP_IDS),
    );
    act(() => result.current.enterSelectionMode());
    expect(result.current.state.selectionMode).toBe(true);
    expect(result.current.state.selectedIds.size).toBe(0);

    act(() => result.current.toggleSelected("a"));
    expect(result.current.state.selectedIds.has("a")).toBe(true);

    act(() => result.current.selectAllVisible());
    expect(result.current.state.selectedIds.size).toBe(3);

    act(() => result.current.clearSelection());
    expect(result.current.state.selectedIds.size).toBe(0);

    act(() => result.current.exitSelectionMode());
    expect(result.current.state.selectionMode).toBe(false);
  });

  it("sourceOptions stays in sync with skills", () => {
    const { result, rerender } = renderHook(
      ({ list }: { list: InstalledSkill[] }) =>
        useSkillsFilterSort(list, {}, APP_IDS),
      { initialProps: { list: skills } },
    );
    expect(result.current.sourceOptions).toEqual([
      "x/y",
      LOCAL_SOURCE_KEY,
    ]);

    rerender({
      list: [
        ...skills,
        makeSkill({ id: "d", repoOwner: "z", repoName: "z" }),
      ],
    });
    expect(result.current.sourceOptions).toEqual([
      "x/y",
      "z/z",
      LOCAL_SOURCE_KEY,
    ]);
  });

  it("setSortKey and setGroupKey update state without other changes", () => {
    const { result } = renderHook(() =>
      useSkillsFilterSort(skills, {}, APP_IDS),
    );
    act(() => {
      result.current.setSortKey("installedNewest");
      result.current.setGroupKey("source");
    });
    expect(result.current.state.sortKey).toBe("installedNewest");
    expect(result.current.state.groupKey).toBe("source");
    expect(result.current.state.searchQuery).toBe("");
  });
});

// ---- helpers ----

function appsWith(enabled: AppId[]): InstalledSkill["apps"] {
  return {
    claude: enabled.includes("claude"),
    codex: enabled.includes("codex"),
    gemini: enabled.includes("gemini"),
    opencode: enabled.includes("opencode"),
    openclaw: enabled.includes("openclaw"),
    hermes: enabled.includes("hermes"),
  };
}
