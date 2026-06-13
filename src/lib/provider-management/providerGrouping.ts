import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { extractProviderSummary } from "@/lib/provider-management/providerSummary";

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

export const buildProviderGroups = (
  providers: Provider[],
  appId: AppId,
): ProviderDisplayGroup[] => {
  const buckets = new Map<
    string,
    { label: string; providers: Provider[]; explicit: boolean }
  >();

  providers.forEach((provider) => {
    const explicitGroup = providerGroupName(provider);
    const summary = extractProviderSummary(provider, appId);
    const key = explicitGroup
      ? `group:${normalizeGroupKey(explicitGroup)}`
      : shouldGroupByHost(provider) && summary.baseUrlHost
        ? `host:${summary.baseUrlHost.toLowerCase()}`
        : `provider:${provider.id}`;
    const label =
      explicitGroup ??
      (key.startsWith("host:") ? (summary.baseUrlHost ?? provider.name) : provider.name);

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
