import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { extractProviderSummary } from "@/lib/provider-management/providerSummary";

export interface ProviderSortUpdate {
  id: string;
  sortIndex: number;
}

export interface ProviderDisplayGroup {
  id: string;
  label: string;
  providers: Provider[];
  primaryProvider: Provider;
  isGrouped: boolean;
}

const GROUPABLE_CATEGORIES = new Set([
  "aggregator",
  "third_party",
  "custom",
  "cloud_provider",
  "cn_official",
  "official",
]);

const normalizeGroupKey = (value: string) =>
  value.trim().toLowerCase().replace(/\s+/g, "-");

const providerGroupName = (provider: Provider) => {
  const group = provider.meta?.providerGroup?.trim();
  return group || undefined;
};

const shouldGroupByHost = (provider: Provider) => {
  if (!provider.category) return true;
  return GROUPABLE_CATEGORIES.has(provider.category);
};

const providerNameGroup = (provider: Provider) => {
  const name = provider.name.trim();
  return name || undefined;
};

export const buildProviderGroups = (
  providers: Provider[],
  appId: AppId,
): ProviderDisplayGroup[] => {
  const nameCounts = new Map<string, number>();
  providers.forEach((provider) => {
    if (!shouldGroupByHost(provider)) return;
    const nameGroup = providerNameGroup(provider);
    if (!nameGroup) return;
    const key = normalizeGroupKey(nameGroup);
    nameCounts.set(key, (nameCounts.get(key) ?? 0) + 1);
  });

  const buckets = new Map<
    string,
    { label: string; providers: Provider[]; explicit: boolean }
  >();

  providers.forEach((provider) => {
    const explicitGroup = providerGroupName(provider);
    const nameGroup = providerNameGroup(provider);
    const nameGroupKey = nameGroup ? normalizeGroupKey(nameGroup) : undefined;
    const shouldUseNameGroup =
      nameGroupKey !== undefined && (nameCounts.get(nameGroupKey) ?? 0) > 1;
    const summary = extractProviderSummary(provider, appId);
    const key = explicitGroup
      ? `group:${normalizeGroupKey(explicitGroup)}`
      : shouldUseNameGroup
        ? `name:${nameGroupKey}`
        : shouldGroupByHost(provider) && summary.baseUrlHost
          ? `host:${summary.baseUrlHost.toLowerCase()}`
          : `provider:${provider.id}`;
    const label =
      explicitGroup ??
      (key.startsWith("name:")
        ? (nameGroup ?? provider.name)
        : key.startsWith("host:")
          ? (summary.baseUrlHost ?? provider.name)
          : provider.name);

    const existing = buckets.get(key);
    if (existing) {
      existing.providers.push(provider);
      if (explicitGroup) {
        existing.explicit = true;
        existing.label = explicitGroup;
      }
    } else {
      buckets.set(key, {
        label,
        providers: [provider],
        explicit: Boolean(explicitGroup),
      });
    }
  });

  return Array.from(buckets.entries()).map(([key, bucket]) => {
    const isGrouped = bucket.explicit || bucket.providers.length > 1;
    return {
      id: key.startsWith("provider:") ? bucket.providers[0].id : key,
      label: isGrouped ? bucket.label : bucket.providers[0].name,
      providers: bucket.providers,
      primaryProvider: bucket.providers[0],
      isGrouped,
    };
  });
};

const moveArrayItem = <T>(items: T[], oldIndex: number, newIndex: number) => {
  const next = [...items];
  const [moved] = next.splice(oldIndex, 1);
  next.splice(newIndex, 0, moved);
  return next;
};

export const buildProviderGroupSortUpdates = (
  groups: ProviderDisplayGroup[],
  activeId: unknown,
  overId: unknown,
  visibleGroups: ProviderDisplayGroup[] = groups,
): ProviderSortUpdate[] => {
  if (overId === undefined || overId === null) return [];

  const activeKey = String(activeId);
  const overKey = String(overId);
  if (activeKey === overKey) return [];

  const oldIndex = visibleGroups.findIndex((group) => group.id === activeKey);
  const newIndex = visibleGroups.findIndex((group) => group.id === overKey);
  if (oldIndex === -1 || newIndex === -1) return [];

  const resolvedVisibleGroups = visibleGroups.map((visibleGroup) => {
    const visibleProviderIds = new Set(
      visibleGroup.providers.map((provider) => provider.id),
    );
    return (
      groups.find((group) => group.id === visibleGroup.id) ??
      groups.find((group) =>
        group.providers.some((provider) => visibleProviderIds.has(provider.id)),
      ) ??
      visibleGroup
    );
  });
  const reorderedVisibleGroups = moveArrayItem(
    resolvedVisibleGroups,
    oldIndex,
    newIndex,
  );
  const visibleGroupIds = new Set(
    resolvedVisibleGroups.map((group) => group.id),
  );
  let nextVisibleIndex = 0;
  const reorderedGroups = groups.map((group) => {
    if (!visibleGroupIds.has(group.id)) return group;
    return reorderedVisibleGroups[nextVisibleIndex++];
  });

  let sortIndex = 0;
  return reorderedGroups.flatMap((group) =>
    group.providers.map((provider) => ({
      id: provider.id,
      sortIndex: sortIndex++,
    })),
  );
};
