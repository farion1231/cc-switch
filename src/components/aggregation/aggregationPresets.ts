import { providerPresets, type ProviderPreset } from "@/config/claudeProviderPresets";
import { codexProviderPresets, type CodexProviderPreset } from "@/config/codexProviderPresets";
import { geminiProviderPresets, type GeminiProviderPreset } from "@/config/geminiProviderPresets";
import {
  PRESET_FAMILIES,
  type PresetFamilyId,
  type PresetPlanKey,
  type PresetRegionKey,
} from "@/config/presetFamilies";
import { PRESET_SEARCH_ALIASES } from "@/config/presetSearchAliases";
import { universalProviderPresets } from "@/config/universalProviderPresets";
import { extractCodexBaseUrl, extractCodexModelName } from "@/utils/providerConfigUtils";
import type {
  ProviderCategory,
  ProviderMeta,
  UniversalProvider,
  UniversalProviderApps,
} from "@/types";

export type AggregationPresetGroupKind =
  | "gateway"
  | "vendor"
  | "relay"
  | "local";

export interface AggregationPresetVariant {
  id: string;
  name: string;
  nameKey?: string;
  providerType: string;
  group: AggregationPresetGroupKind;
  family?: PresetFamilyId;
  planKey?: PresetPlanKey;
  regionKey?: PresetRegionKey;
  baseUrl: string;
  apiFormat: "anthropic" | "openai_chat" | "openai_responses" | "gemini_native";
  defaultModel: string;
  extraModels: string[];
  websiteUrl?: string;
  apiKeyUrl?: string;
  icon?: string;
  iconColor?: string;
  description?: string;
  isCustomTemplate?: boolean;
  keyOptional?: boolean;
  searchText: string;
}

export interface AggregationPresetEntry {
  id: string;
  name: string;
  nameKey?: string;
  family?: PresetFamilyId;
  group: AggregationPresetGroupKind;
  icon?: string;
  iconColor?: string;
  variants: AggregationPresetVariant[];
  searchText: string;
}

function isLoopbackUrl(url: string): boolean {
  const lower = url.trim().toLowerCase();
  return (
    lower.startsWith("http://localhost") ||
    lower.startsWith("https://localhost") ||
    lower.startsWith("http://127.0.0.1") ||
    lower.startsWith("https://127.0.0.1")
  );
}

export function extractHost(url?: string): string {
  if (!url) return "";
  try {
    return new URL(url).host;
  } catch {
    return url.replace(/^https?:\/\//i, "").split("/")[0] ?? url;
  }
}

export function maskSecretKey(raw?: string): string {
  const key = raw?.trim() ?? "";
  if (!key) return "";
  if (key === "local-no-key" || key === "ollama" || key === "lmstudio" || key === "no-key") {
    return "";
  }
  if (key.length <= 8) {
    return `${key.slice(0, 2)}••••`;
  }
  return `${key.slice(0, 4)}•••${key.slice(-4)}`;
}

function mapCategoryToGroup(
  category?: ProviderCategory,
  isCustomTemplate?: boolean,
): AggregationPresetGroupKind {
  if (isCustomTemplate) return "gateway";
  if (category === "local") return "local";
  if (category === "cn_official" || category === "cloud_provider") {
    return "vendor";
  }
  return "relay";
}

function normalizeUrlCandidate(raw?: string): string {
  if (!raw) return "";
  const trimmed = raw.trim().replace(/\/+$/, "");
  if (!/^https?:\/\//i.test(trimmed)) return "";
  if (trimmed.includes("${")) return "";
  return trimmed;
}

function buildSearchText(
  name: string,
  baseUrl: string,
  familyName?: string,
  extra?: string,
): string {
  const alias = PRESET_SEARCH_ALIASES[name] ?? "";
  const host = extractHost(baseUrl);
  return [name, familyName ?? "", host, baseUrl, alias, extra ?? ""]
    .join(" ")
    .toLowerCase();
}

function collectClaudeVariants(): AggregationPresetVariant[] {
  const list: AggregationPresetVariant[] = [];
  for (const preset of providerPresets) {
    if (
      preset.hidden ||
      preset.isOfficial ||
      preset.requiresOAuth ||
      preset.category === "official"
    ) {
      continue;
    }
    const env =
      (preset.settingsConfig as { env?: Record<string, string> })?.env ?? {};
    const baseUrl = normalizeUrlCandidate(
      preset.endpointCandidates?.[0] || env.ANTHROPIC_BASE_URL,
    );
    if (!baseUrl) continue;

    const models = [
      env.ANTHROPIC_MODEL,
      env.ANTHROPIC_DEFAULT_SONNET_MODEL,
      env.ANTHROPIC_DEFAULT_OPUS_MODEL,
      env.ANTHROPIC_DEFAULT_HAIKU_MODEL,
    ].filter((m, idx, arr): m is string => Boolean(m) && arr.indexOf(m) === idx);

    const apiFormat = preset.apiFormat ?? "anthropic";
    const familyName = preset.family
      ? PRESET_FAMILIES[preset.family]?.name
      : undefined;

    list.push({
      id: `claude:${preset.name}`,
      name: preset.name,
      nameKey: preset.nameKey,
      providerType: preset.name.toLowerCase().replace(/[^a-z0-9]+/g, "_"),
      group: mapCategoryToGroup(preset.category),
      family: preset.family,
      planKey: preset.planKey,
      regionKey: preset.regionKey,
      baseUrl,
      apiFormat,
      defaultModel: models[0] ?? "claude-sonnet-4-20250514",
      extraModels: models.slice(1),
      websiteUrl: preset.websiteUrl,
      apiKeyUrl: preset.apiKeyUrl,
      icon: preset.icon ?? preset.theme?.icon,
      iconColor: preset.iconColor,
      keyOptional: preset.category === "local" || isLoopbackUrl(baseUrl),
      searchText: buildSearchText(preset.name, baseUrl, familyName),
    });
  }
  return list;
}

function collectCodexOnlyVariants(
  existingNames: Set<string>,
): AggregationPresetVariant[] {
  const list: AggregationPresetVariant[] = [];
  for (const preset of codexProviderPresets as CodexProviderPreset[]) {
    if (
      preset.hidden ||
      preset.isOfficial ||
      preset.requiresOAuth ||
      preset.category === "official" ||
      preset.isCustomTemplate
    ) {
      continue;
    }
    if (existingNames.has(preset.name.toLowerCase())) continue;

    const baseUrl = normalizeUrlCandidate(
      preset.endpointCandidates?.[0] || extractCodexBaseUrl(preset.config),
    );
    if (!baseUrl) continue;

    const defaultModel = extractCodexModelName(preset.config) || "gpt-4o";
    const apiFormat = preset.apiFormat ?? "openai_responses";
    const familyName = preset.family
      ? PRESET_FAMILIES[preset.family]?.name
      : undefined;

    list.push({
      id: `codex:${preset.name}`,
      name: preset.name,
      nameKey: preset.nameKey,
      providerType: preset.name.toLowerCase().replace(/[^a-z0-9]+/g, "_"),
      group: mapCategoryToGroup(preset.category),
      family: preset.family,
      planKey: preset.planKey,
      regionKey: preset.regionKey,
      baseUrl,
      apiFormat,
      defaultModel,
      extraModels: (preset.modelCatalog ?? [])
        .map((m) => m.model)
        .filter((m) => m && m !== defaultModel),
      websiteUrl: preset.websiteUrl,
      apiKeyUrl: preset.apiKeyUrl,
      icon: preset.icon ?? preset.theme?.icon,
      iconColor: preset.iconColor,
      keyOptional: preset.category === "local" || isLoopbackUrl(baseUrl),
      searchText: buildSearchText(preset.name, baseUrl, familyName),
    });
  }
  return list;
}

function collectGeminiOnlyVariants(
  existingNames: Set<string>,
): AggregationPresetVariant[] {
  const list: AggregationPresetVariant[] = [];
  for (const preset of geminiProviderPresets as GeminiProviderPreset[]) {
    if (preset.category === "official") continue;
    if (existingNames.has(preset.name.toLowerCase())) continue;

    const env =
      (preset.settingsConfig as { env?: Record<string, string> })?.env ?? {};
    const baseUrl = normalizeUrlCandidate(
      preset.endpointCandidates?.[0] || preset.baseURL || env.GOOGLE_GEMINI_BASE_URL,
    );
    if (!baseUrl) continue;

    const defaultModel = preset.model || env.GEMINI_MODEL || "gemini-2.5-pro";
    const familyName = preset.family
      ? PRESET_FAMILIES[preset.family]?.name
      : undefined;

    list.push({
      id: `gemini:${preset.name}`,
      name: preset.name,
      nameKey: preset.nameKey,
      providerType: preset.name.toLowerCase().replace(/[^a-z0-9]+/g, "_"),
      group: mapCategoryToGroup(preset.category),
      family: preset.family,
      planKey: preset.planKey,
      regionKey: preset.regionKey,
      baseUrl,
      apiFormat: "gemini_native",
      defaultModel,
      extraModels: [],
      websiteUrl: preset.websiteUrl,
      apiKeyUrl: preset.apiKeyUrl,
      icon: preset.icon ?? preset.theme?.icon,
      iconColor: preset.iconColor,
      keyOptional: preset.category === "local" || isLoopbackUrl(baseUrl),
      searchText: buildSearchText(preset.name, baseUrl, familyName),
    });
  }
  return list;
}

let cachedCatalog: AggregationPresetEntry[] | null = null;

export function getAggregationPresetCatalog(): AggregationPresetEntry[] {
  if (cachedCatalog) return cachedCatalog;

  const variants: AggregationPresetVariant[] = [];

  for (const u of universalProviderPresets) {
    variants.push({
      id: `universal:${u.providerType}`,
      name: u.name,
      providerType: u.providerType,
      group: "gateway",
      baseUrl: "",
      apiFormat: "openai_chat",
      defaultModel:
        u.defaultModels.codex?.model ||
        u.defaultModels.claude?.model ||
        "gpt-4o",
      extraModels: [],
      websiteUrl: u.websiteUrl,
      icon: u.icon,
      iconColor: u.iconColor,
      description: u.description,
      isCustomTemplate: u.isCustomTemplate ?? u.providerType === "newapi",
      searchText: buildSearchText(u.name, u.websiteUrl ?? "", undefined, u.description),
    });
  }

  const claudeVariants = collectClaudeVariants();
  variants.push(...claudeVariants);

  const seenNames = new Set(variants.map((v) => v.name.toLowerCase()));
  const codexVariants = collectCodexOnlyVariants(seenNames);
  variants.push(...codexVariants);
  for (const v of codexVariants) seenNames.add(v.name.toLowerCase());

  const geminiVariants = collectGeminiOnlyVariants(seenNames);
  variants.push(...geminiVariants);

  // 将同 family 的变体合成为单个条目（对照 Magpie pairTile / CC Switch PRESET_FAMILIES）
  const entries: AggregationPresetEntry[] = [];
  const familyBuckets = new Map<PresetFamilyId, AggregationPresetVariant[]>();

  for (const v of variants) {
    if (v.family && PRESET_FAMILIES[v.family]) {
      const bucket = familyBuckets.get(v.family) ?? [];
      bucket.push(v);
      familyBuckets.set(v.family, bucket);
    } else {
      entries.push({
        id: v.id,
        name: v.name,
        nameKey: v.nameKey,
        group: v.group,
        icon: v.icon,
        iconColor: v.iconColor,
        variants: [v],
        searchText: v.searchText,
      });
    }
  }

  for (const [familyId, groupVariants] of familyBuckets) {
    const info = PRESET_FAMILIES[familyId];
    const lead = groupVariants[0];
    entries.push({
      id: `family:${familyId}`,
      name: info.name,
      nameKey: "nameKey" in info ? info.nameKey : undefined,
      family: familyId,
      group: lead.group,
      icon: lead.icon,
      iconColor: lead.iconColor,
      variants: groupVariants,
      searchText: groupVariants.map((v) => v.searchText).join(" "),
    });
  }

  // 把最常用的放在各分组前面，保持列表整齐直观
  const POPULAR_NAMES = [
    "NewAPI",
    "自定义网关",
    "DeepSeek",
    "Kimi",
    "Zhipu GLM",
    "MiniMax",
    "QwenCloud",
    "Volcengine",
    "OpenRouter",
    "SiliconFlow",
    "AiHubMix",
    "Ollama",
    "LM Studio",
  ];
  const popularityRank = (name: string) => {
    const idx = POPULAR_NAMES.indexOf(name);
    return idx === -1 ? 999 : idx;
  };
  const groupOrder: Record<AggregationPresetGroupKind, number> = {
    gateway: 0,
    vendor: 1,
    relay: 2,
    local: 3,
  };

  entries.sort((a, b) => {
    const go = groupOrder[a.group] - groupOrder[b.group];
    if (go !== 0) return go;
    const po = popularityRank(a.name) - popularityRank(b.name);
    if (po !== 0) return po;
    return a.name.localeCompare(b.name);
  });

  cachedCatalog = entries;
  return entries;
}

export function getFeaturedAggregationPresets(): AggregationPresetEntry[] {
  const catalog = getAggregationPresetCatalog();
  const wanted = [
    "DeepSeek",
    "Kimi",
    "Zhipu GLM",
    "OpenRouter",
    "NewAPI",
    "自定义网关",
  ];
  const picked: AggregationPresetEntry[] = [];
  for (const target of wanted) {
    const found = catalog.find(
      (item) =>
        item.name === target ||
        item.variants.some((v) => v.name === target),
    );
    if (found && !picked.includes(found)) {
      picked.push(found);
    }
  }
  return picked;
}

export function buildUniversalFromAggregationInput(params: {
  id?: string;
  name: string;
  providerType: string;
  baseUrl: string;
  apiKey: string;
  apiFormat: string;
  primaryModel: string;
  extraModels: string[];
  apps: UniversalProviderApps;
  reasoningEffort?: string;
  haikuModel?: string;
  sonnetModel?: string;
  opusModel?: string;
  meta?: ProviderMeta;
  websiteUrl?: string;
  notes?: string;
  icon?: string;
  iconColor?: string;
  existing?: UniversalProvider | null;
}): UniversalProvider {
  const model = params.primaryModel.trim() || "gpt-4o";
  const key = params.apiKey.trim() || "local-no-key";
  const dedupedExtra: string[] = [];
  for (const raw of params.extraModels) {
    const trimmed = raw.trim();
    if (trimmed && trimmed !== model && !dedupedExtra.includes(trimmed)) {
      dedupedExtra.push(trimmed);
    }
  }
  const mergedMeta: ProviderMeta | undefined = params.meta
    ? {
        ...(params.existing?.meta ?? {}),
        ...params.meta,
      }
    : params.existing?.meta;

  return {
    id: params.id ?? params.existing?.id ?? crypto.randomUUID(),
    name: params.name.trim(),
    providerType: params.providerType,
    apps: params.apps,
    baseUrl: params.baseUrl.trim().replace(/\/+$/, ""),
    apiKey: key,
    apiFormat: params.apiFormat,
    extraModels: dedupedExtra,
    models: {
      claude: {
        model,
        haikuModel: params.haikuModel?.trim() || model,
        sonnetModel: params.sonnetModel?.trim() || model,
        opusModel: params.opusModel?.trim() || model,
      },
      codex: {
        model,
        reasoningEffort:
          params.reasoningEffort?.trim() ||
          params.existing?.models?.codex?.reasoningEffort ||
          "high",
      },
      gemini: {
        model,
      },
    },
    websiteUrl: params.websiteUrl?.trim() || undefined,
    notes: params.notes?.trim() || undefined,
    icon: params.icon ?? params.existing?.icon,
    iconColor: params.iconColor ?? params.existing?.iconColor,
    meta: mergedMeta,
    createdAt: params.existing?.createdAt ?? Date.now(),
    sortIndex: params.existing?.sortIndex,
  };
}

export function findPresetVariantForUniversal(
  provider?: UniversalProvider | null,
): { entry: AggregationPresetEntry; variant: AggregationPresetVariant } | null {
  if (!provider) return null;
  const catalog = getAggregationPresetCatalog();
  for (const entry of catalog) {
    for (const variant of entry.variants) {
      if (variant.providerType === provider.providerType) {
        return { entry, variant };
      }
    }
  }
  return null;
}

export function getPresetVariantLabel(
  variant: AggregationPresetVariant,
  t: (key: string, opts?: { defaultValue?: string }) => string,
): string {
  const parts: string[] = [];
  if (variant.regionKey) {
    parts.push(
      t(`providerPreset.region.${variant.regionKey}`, {
        defaultValue: variant.regionKey === "cn" ? "国内" : "海外",
      }),
    );
  }
  if (variant.planKey) {
    parts.push(
      t(`providerPreset.plan.${variant.planKey}`, {
        defaultValue: variant.planKey,
      }),
    );
  }
  if (parts.length > 0) return parts.join(" · ");
  const host = extractHost(variant.baseUrl);
  return host || variant.name;
}

export type { ProviderPreset };
