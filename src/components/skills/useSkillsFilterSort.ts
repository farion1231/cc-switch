import { useCallback, useMemo, useState } from "react";
import type { AppId } from "@/lib/api/types";
import type { InstalledSkill, SkillUpdateInfo } from "@/lib/api/skills";

export type SortKey =
  | "nameAsc"
  | "nameDesc"
  | "installedNewest"
  | "installedOldest"
  | "sourceAsc";

export type GroupKey = "none" | "source" | "app";

export interface FilterSortState {
  searchQuery: string;
  filterSources: Set<string>;
  filterApps: Set<AppId>;
  filterUpdateOnly: boolean;
  sortKey: SortKey;
  groupKey: GroupKey;
  collapsed: Set<string>;
  selectionMode: boolean;
  selectedIds: Set<string>;
}

export interface SkillGroup {
  key: string;
  label: string;
  items: InstalledSkill[];
}

export const LOCAL_SOURCE_KEY = "__local__";
export const ALL_GROUP_KEY = "__all__";

/** 组成 skill 的来源 key（owner/name 或 LOCAL_SOURCE_KEY） */
export function getSourceKey(skill: InstalledSkill): string {
  if (skill.repoOwner && skill.repoName) {
    return `${skill.repoOwner}/${skill.repoName}`;
  }
  return LOCAL_SOURCE_KEY;
}

/** 全部独立的搜索匹配：name/description/directory/owner/name 任意命中 */
export function matchSkillBySearch(
  skill: InstalledSkill,
  query: string,
): boolean {
  if (!query) return true;
  const q = query.toLowerCase();
  return (
    skill.name.toLowerCase().includes(q) ||
    (skill.description?.toLowerCase().includes(q) ?? false) ||
    skill.directory.toLowerCase().includes(q) ||
    (skill.repoOwner?.toLowerCase().includes(q) ?? false) ||
    (skill.repoName?.toLowerCase().includes(q) ?? false) ||
    `${skill.repoOwner ?? ""}/${skill.repoName ?? ""}`.toLowerCase().includes(q)
  );
}

const SORT_FNS: Record<
  SortKey,
  (a: InstalledSkill, b: InstalledSkill) => number
> = {
  nameAsc: (a, b) => a.name.localeCompare(b.name),
  nameDesc: (a, b) => b.name.localeCompare(a.name),
  installedNewest: (a, b) => b.installedAt - a.installedAt,
  installedOldest: (a, b) => a.installedAt - b.installedAt,
  sourceAsc: (a, b) => getSourceKey(a).localeCompare(getSourceKey(b)),
};

/** 应用排序，置顶项始终在前；置顶之间按 pinnedAt 降序 */
export function sortSkills(
  skills: InstalledSkill[],
  sortKey: SortKey,
): InstalledSkill[] {
  const cmp = SORT_FNS[sortKey];
  return [...skills].sort((a, b) => {
    const pa = a.pinnedAt ?? 0;
    const pb = b.pinnedAt ?? 0;
    if (pa !== pb) return pb - pa;
    return cmp(a, b);
  });
}

/** 仅做搜索 + 过滤（不排序、不分组） */
export function filterSkills(
  skills: InstalledSkill[],
  state: Pick<
    FilterSortState,
    "searchQuery" | "filterSources" | "filterApps" | "filterUpdateOnly"
  >,
  updatesMap: Record<string, SkillUpdateInfo>,
): InstalledSkill[] {
  const q = state.searchQuery.trim();
  return skills.filter((skill) => {
    if (!matchSkillBySearch(skill, q)) return false;

    if (state.filterSources.size > 0) {
      if (!state.filterSources.has(getSourceKey(skill))) return false;
    }

    if (state.filterApps.size > 0) {
      const hit = [...state.filterApps].some(
        (app) => skill.apps[app as keyof typeof skill.apps] === true,
      );
      if (!hit) return false;
    }

    if (state.filterUpdateOnly) {
      if (!updatesMap[skill.id]) return false;
    }

    return true;
  });
}

/** 应用分组（不分组返回单一 group） */
export function groupSkills(
  skills: InstalledSkill[],
  groupKey: GroupKey,
  appIds: AppId[],
): SkillGroup[] {
  if (groupKey === "none") {
    return [{ key: ALL_GROUP_KEY, label: "", items: skills }];
  }

  if (groupKey === "source") {
    const map = new Map<string, InstalledSkill[]>();
    for (const s of skills) {
      const key = getSourceKey(s);
      const arr = map.get(key) ?? [];
      arr.push(s);
      map.set(key, arr);
    }
    return [...map.entries()]
      .sort(([a], [b]) => {
        if (a === LOCAL_SOURCE_KEY) return 1;
        if (b === LOCAL_SOURCE_KEY) return -1;
        return a.localeCompare(b);
      })
      .map(([key, items]) => ({
        key,
        label: key === LOCAL_SOURCE_KEY ? "" : key,
        items,
      }));
  }

  // groupKey === "app"：同一 skill 可能出现在多组
  return appIds.map((app) => ({
    key: app,
    label: app,
    items: skills.filter((s) => s.apps[app as keyof typeof s.apps] === true),
  }));
}

/** 完整流水线：过滤 → 排序 → 分组 */
export function computeGroups(
  skills: InstalledSkill[],
  state: Pick<
    FilterSortState,
    | "searchQuery"
    | "filterSources"
    | "filterApps"
    | "filterUpdateOnly"
    | "sortKey"
    | "groupKey"
  >,
  updatesMap: Record<string, SkillUpdateInfo>,
  appIds: AppId[],
): { filtered: InstalledSkill[]; groups: SkillGroup[] } {
  const filtered = filterSkills(skills, state, updatesMap);
  const sorted = sortSkills(filtered, state.sortKey);
  const groups = groupSkills(sorted, state.groupKey, appIds);
  return { filtered, groups };
}

const DEFAULT_STATE: FilterSortState = {
  searchQuery: "",
  filterSources: new Set(),
  filterApps: new Set(),
  filterUpdateOnly: false,
  sortKey: "nameAsc",
  groupKey: "none",
  collapsed: new Set(),
  selectionMode: false,
  selectedIds: new Set(),
};

/** 派生出当前 skills 中可用的来源 key 列表（已排序，含 LOCAL_SOURCE_KEY 时排末尾） */
export function deriveSourceOptions(skills: InstalledSkill[]): string[] {
  const set = new Set<string>();
  for (const s of skills) {
    set.add(getSourceKey(s));
  }
  return [...set].sort((a, b) => {
    if (a === LOCAL_SOURCE_KEY) return 1;
    if (b === LOCAL_SOURCE_KEY) return -1;
    return a.localeCompare(b);
  });
}

/**
 * Skills 列表的搜索/过滤/排序/分组/多选/置顶 hook
 *
 * 状态全部内部管理，对外暴露派生数据与 setters。
 */
export function useSkillsFilterSort(
  skills: InstalledSkill[],
  updatesMap: Record<string, SkillUpdateInfo>,
  appIds: AppId[],
) {
  const [state, setState] = useState<FilterSortState>(DEFAULT_STATE);

  const { filtered, groups } = useMemo(
    () => computeGroups(skills, state, updatesMap, appIds),
    [skills, state, updatesMap, appIds],
  );

  const sourceOptions = useMemo(() => deriveSourceOptions(skills), [skills]);

  const setSearchQuery = useCallback((q: string) => {
    setState((s) => ({ ...s, searchQuery: q }));
  }, []);

  const toggleSource = useCallback((key: string) => {
    setState((s) => {
      const next = new Set(s.filterSources);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return { ...s, filterSources: next };
    });
  }, []);

  const toggleApp = useCallback((app: AppId) => {
    setState((s) => {
      const next = new Set(s.filterApps);
      if (next.has(app)) next.delete(app);
      else next.add(app);
      return { ...s, filterApps: next };
    });
  }, []);

  const setFilterUpdateOnly = useCallback((v: boolean) => {
    setState((s) => ({ ...s, filterUpdateOnly: v }));
  }, []);

  const setSortKey = useCallback((k: SortKey) => {
    setState((s) => ({ ...s, sortKey: k }));
  }, []);

  const setGroupKey = useCallback((k: GroupKey) => {
    setState((s) => ({ ...s, groupKey: k }));
  }, []);

  const toggleCollapsed = useCallback((key: string) => {
    setState((s) => {
      const next = new Set(s.collapsed);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return { ...s, collapsed: next };
    });
  }, []);

  const clearFilters = useCallback(() => {
    setState((s) => ({
      ...s,
      searchQuery: "",
      filterSources: new Set(),
      filterApps: new Set(),
      filterUpdateOnly: false,
    }));
  }, []);

  const enterSelectionMode = useCallback(() => {
    setState((s) => ({ ...s, selectionMode: true, selectedIds: new Set() }));
  }, []);

  const exitSelectionMode = useCallback(() => {
    setState((s) => ({ ...s, selectionMode: false, selectedIds: new Set() }));
  }, []);

  const toggleSelected = useCallback((id: string) => {
    setState((s) => {
      const next = new Set(s.selectedIds);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return { ...s, selectedIds: next };
    });
  }, []);

  const selectAllVisible = useCallback(() => {
    setState((s) => ({
      ...s,
      selectedIds: new Set(filtered.map((sk) => sk.id)),
    }));
  }, [filtered]);

  const clearSelection = useCallback(() => {
    setState((s) => ({ ...s, selectedIds: new Set() }));
  }, []);

  const hasFilters =
    state.searchQuery.trim().length > 0 ||
    state.filterSources.size > 0 ||
    state.filterApps.size > 0 ||
    state.filterUpdateOnly;

  return {
    state,
    filtered,
    groups,
    sourceOptions,
    total: skills.length,
    filteredCount: filtered.length,
    hasFilters,
    setSearchQuery,
    toggleSource,
    toggleApp,
    setFilterUpdateOnly,
    setSortKey,
    setGroupKey,
    toggleCollapsed,
    clearFilters,
    enterSelectionMode,
    exitSelectionMode,
    toggleSelected,
    selectAllVisible,
    clearSelection,
  };
}
