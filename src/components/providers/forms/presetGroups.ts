import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";
import {
  PRESET_FAMILIES,
  PRESET_REGION_KEYS,
  type PresetFamilyId,
  type PresetFamilyInfo,
  type PresetPlanKey,
  type PresetRegionKey,
} from "@/config/presetFamilies";
import { PRESET_SEARCH_ALIASES } from "@/config/presetSearchAliases";
import { providerNeedsRouting } from "@/utils/providerCapabilities";
import type { AnyPreset, PresetEntry } from "./ProviderPresetSelector";

type Translate = (key: string) => unknown;

/**
 * 添加供应商时左侧的分类（v7）：只在显示时按预设现有字段推算，不改 `category`
 * （很多行为依赖它）。
 */
export type PresetGroup =
  | "login"
  | "vendor"
  | "thirdparty"
  | "cloud"
  | "plugin";

export const PRESET_GROUP_ORDER: PresetGroup[] = [
  "login",
  "vendor",
  "thirdparty",
  "cloud",
  "plugin",
];

type PresetFields = AnyPreset & {
  websiteUrl?: string;
  requiresOAuth?: boolean;
  providerType?: string;
  apiFormat?: string;
};

export function presetGroup(preset: AnyPreset): PresetGroup {
  const p = preset as PresetFields;
  if (p.category === "official" || p.requiresOAuth || p.providerType) {
    return "login";
  }
  switch (p.category) {
    case "cn_official":
      return "vendor";
    case "cloud_provider":
      return "cloud";
    case "omo":
    case "omo-slim":
      return "plugin";
    default:
      return "thirdparty";
  }
}

/** 网址的主机名（去掉 www.） */
export function presetDomain(preset: AnyPreset): string {
  const url = (preset as PresetFields).websiteUrl;
  if (!url) return "";
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return "";
  }
}

const DOMAIN_PREFIXES =
  /^(www|api|platform|open|console|cloud|dashboard|app)\./;
const DOMAIN_SUFFIXES =
  /(\.(com|cn|ai|io|net|org|dev|app|top|xyz|cc|co|me|tech|site|pro|vip|us|hk|jp))+$/;

/** 域名主体：去掉 www/api 等前缀和 .com/.cn 等后缀，免得搜「com」「api」全命中 */
export function domainBody(domain: string): string {
  return domain.replace(DOMAIN_PREFIXES, "").replace(DOMAIN_SUFFIXES, "");
}

export function presetDisplayName(preset: AnyPreset, t: Translate): string {
  return preset.nameKey ? String(t(preset.nameKey)) : preset.name;
}

/**
 * 每个词都要出现在 `texts` 里；带点的词（「kimi.ai」「z.ai」）也可以命中完整域名。
 * 域名平时只比主体，免得「com」「api」全命中。
 */
function fieldsMatch(
  query: string,
  texts: string[],
  domains: string[] = [],
): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  const haystack = texts.join(" ").toLowerCase();
  const hosts = domains.filter(Boolean).map((domain) => domain.toLowerCase());
  return needle
    .split(/\s+/)
    .every(
      (part) =>
        part.length > 0 &&
        (haystack.includes(part) ||
          (part.includes(".") && hosts.some((host) => host.includes(part)))),
    );
}

export function presetMatches(
  entry: PresetEntry,
  query: string,
  t: Translate,
): boolean {
  const domain = presetDomain(entry.preset);
  return fieldsMatch(
    query,
    [
      presetDisplayName(entry.preset, t),
      entry.preset.name,
      domainBody(domain),
      PRESET_SEARCH_ALIASES[entry.preset.name] ?? "",
    ],
    [domain],
  );
}

// ─── 厂商 / 版本：同一家的多个版本在第 1 步合成一行 ────────────────────────────

/** 第 1 步的一行：一个预设，或者同一家的几个版本（按预设文件里的顺序） */
export interface PresetRowItem {
  key: string;
  /** 只有真有多个版本时才有 */
  family?: PresetFamilyId;
  versions: PresetEntry[];
}

/** 把同一 `family` 的预设合成一行；只剩一个版本的照旧单独一行 */
export function groupPresetRows(entries: PresetEntry[]): PresetRowItem[] {
  const byFamily = new Map<PresetFamilyId, PresetEntry[]>();
  for (const entry of entries) {
    const family = entry.preset.family;
    if (family) byFamily.set(family, [...(byFamily.get(family) ?? []), entry]);
  }
  const rows: PresetRowItem[] = [];
  const done = new Set<PresetFamilyId>();
  for (const entry of entries) {
    const family = entry.preset.family;
    const versions = family ? byFamily.get(family) : undefined;
    if (family && versions && versions.length > 1) {
      if (done.has(family)) continue;
      done.add(family);
      rows.push({
        key: `family:${family}`,
        family,
        versions: sortFamilyVersions(family, versions),
      });
    } else {
      rows.push({ key: entry.id, versions: [entry] });
    }
  }
  return rows;
}

/** 选中的预设所在的那一家（含它自己）；没有别的版本时只有它自己 */
export function presetVersions(
  entries: PresetEntry[],
  entry: PresetEntry,
): PresetEntry[] {
  const family = entry.preset.family;
  if (!family) return [entry];
  const versions = entries.filter((item) => item.preset.family === family);
  return versions.length > 1 ? sortFamilyVersions(family, versions) : [entry];
}

/**
 * 排版本：先按套餐（PRESET_FAMILIES 里的 planOrder，没列到的按文件里第一次出现的顺序），
 * 同一套餐里再按地区（先国内后海外）；都没写的保持文件顺序。
 */
function sortFamilyVersions(
  family: PresetFamilyId,
  versions: PresetEntry[],
): PresetEntry[] {
  const listed: readonly string[] =
    (PRESET_FAMILIES[family] as PresetFamilyInfo).planOrder ?? [];
  const plans: string[] = [...listed];
  for (const entry of versions) {
    const plan = entry.preset.planKey;
    if (plan && !plans.includes(plan)) plans.push(plan);
  }
  const planRank = (entry: PresetEntry) => {
    const plan = entry.preset.planKey;
    return plan ? plans.indexOf(plan) : plans.length;
  };
  const regionRank = (entry: PresetEntry) => {
    const region = entry.preset.regionKey;
    return region ? PRESET_REGION_KEYS.indexOf(region) : -1;
  };
  return [...versions].sort(
    (a, b) => planRank(a) - planRank(b) || regionRank(a) - regionRank(b),
  );
}

export function familyDisplayName(family: PresetFamilyId, t: Translate) {
  const info: PresetFamilyInfo = PRESET_FAMILIES[family];
  return info.nameKey ? String(t(info.nameKey)) : info.name;
}

export function presetRowName(row: PresetRowItem, t: Translate): string {
  return row.family
    ? familyDisplayName(row.family, t)
    : presetDisplayName(row.versions[0].preset, t);
}

export function presetPlanLabel(plan: PresetPlanKey, t: Translate): string {
  return String(t(`providerPreset.plan.${plan}`));
}

export function presetRegionLabel(
  region: PresetRegionKey,
  t: Translate,
): string {
  return String(t(`providerPreset.region.${region}`));
}

/** 版本标签「套餐 · 地区」（「编程订阅 · 国内」）；两个都没写的用域名 */
export function presetVersionLabel(entry: PresetEntry, t: Translate): string {
  const { planKey, regionKey } = entry.preset;
  const parts = [
    planKey ? presetPlanLabel(planKey, t) : "",
    regionKey ? presetRegionLabel(regionKey, t) : "",
  ].filter(Boolean);
  if (parts.length > 0) return parts.join(" · ");
  return presetDomain(entry.preset) || presetDisplayName(entry.preset, t);
}

/**
 * 第 2 步怎么选版本：
 * - `single`：版本只在一个维度上不同（或都没写维度、用域名区分）→ 一个分段控件，
 *   按钮只写变化的那一维（`dimension` 为 null 时写完整标签）；
 * - `grid`：套餐和地区都在变，而且每个套餐 × 每个地区都正好有一个预设 → 套餐、地区各一个分段控件；
 * - `list`：其余（两维都变但缺格子、标签重复等）→ 下拉，列出「套餐 · 地区」。
 */
export type PresetVersionLayout =
  | { kind: "single"; dimension: "plan" | "region" | null }
  | { kind: "grid"; plans: PresetPlanKey[]; regions: PresetRegionKey[] }
  | { kind: "list" };

function distinct<T>(values: T[]): T[] {
  return values.filter((value, index) => values.indexOf(value) === index);
}

/** `versions` 需已排好序（presetVersions / groupPresetRows 的结果） */
export function presetVersionLayout(
  versions: PresetEntry[],
): PresetVersionLayout {
  const planOf = versions.map((entry) => entry.preset.planKey);
  const regionOf = versions.map((entry) => entry.preset.regionKey);
  const plans = distinct(planOf);
  const regions = distinct(regionOf);
  const unique = (values: unknown[]) =>
    distinct(values).length === versions.length;

  if (plans.length > 1 && regions.length > 1) {
    const complete =
      !plans.includes(undefined) &&
      !regions.includes(undefined) &&
      plans.length * regions.length === versions.length &&
      unique(planOf.map((plan, i) => `${plan}|${regionOf[i]}`));
    if (!complete) return { kind: "list" };
    return {
      kind: "grid",
      plans: plans as PresetPlanKey[],
      regions: [...(regions as PresetRegionKey[])].sort(
        (a, b) => PRESET_REGION_KEYS.indexOf(a) - PRESET_REGION_KEYS.indexOf(b),
      ),
    };
  }
  if (plans.length > 1) {
    return !plans.includes(undefined) && unique(planOf)
      ? { kind: "single", dimension: "plan" }
      : { kind: "list" };
  }
  if (regions.length > 1) {
    return !regions.includes(undefined) && unique(regionOf)
      ? { kind: "single", dimension: "region" }
      : { kind: "list" };
  }
  // 两维都没变化：靠域名区分（SudoCode）；连域名都一样就只能下拉
  return unique(versions.map((entry) => presetDomain(entry.preset)))
    ? { kind: "single", dimension: null }
    : { kind: "list" };
}

/** 分段控件按钮上的字：只写变化的那一维 */
export function presetVersionShortLabel(
  entry: PresetEntry,
  dimension: "plan" | "region" | null,
  t: Translate,
): string {
  const { planKey, regionKey } = entry.preset;
  if (dimension === "plan" && planKey) return presetPlanLabel(planKey, t);
  if (dimension === "region" && regionKey) {
    return presetRegionLabel(regionKey, t);
  }
  return presetVersionLabel(entry, t);
}

/**
 * 搜索一行。返回 null 表示不匹配；`versions` 是命中的版本下标，空数组表示整家命中。
 * 顺序照画板：家名 / 主域名命中算整家；否则逐个版本比版本名、版本标签和它自己的域名
 * （kimi.ai、z.ai 这类只属于海外站的域名落到那个版本上）；都没中再看别名，算整家。
 */
export function matchPresetRow(
  row: PresetRowItem,
  query: string,
  t: Translate,
): { versions: number[] } | null {
  if (!query.trim()) return { versions: [] };
  if (!row.family) {
    return presetMatches(row.versions[0], query, t) ? { versions: [] } : null;
  }
  const mainDomain = presetDomain(row.versions[0].preset);
  if (
    fieldsMatch(
      query,
      [familyDisplayName(row.family, t), domainBody(mainDomain)],
      [mainDomain],
    )
  ) {
    return { versions: [] };
  }
  const hits: number[] = [];
  row.versions.forEach((entry, index) => {
    const domain = presetDomain(entry.preset);
    const ownDomain = domain && domain !== mainDomain ? domain : "";
    if (
      fieldsMatch(
        query,
        [
          presetDisplayName(entry.preset, t),
          entry.preset.name,
          presetVersionLabel(entry, t),
          domainBody(ownDomain),
        ],
        [ownDomain],
      )
    ) {
      hits.push(index);
    }
  });
  if (hits.length > 0) return { versions: hits };
  return row.versions.some((entry) => presetMatches(entry, query, t))
    ? { versions: [] }
    : null;
}

// ─── 「全部」按分类分段 ──────────────────────────────────────────────────────

/** 一行的分类：合成的行看第一个版本（同一家的版本分类一致） */
export function presetRowGroup(row: PresetRowItem): PresetGroup {
  return presetGroup(row.versions[0].preset);
}

/**
 * 「全部」按左侧分类分段（账号登录 → 模型厂商 → 第三方平台 → 云服务商 → 插件配置），
 * 段内保持传入的顺序（已按名称排）；空段不出现。
 */
export function sectionPresetRows<T extends { row: PresetRowItem }>(
  items: T[],
): { group: PresetGroup; items: T[] }[] {
  return PRESET_GROUP_ORDER.map((group) => ({
    group,
    items: items.filter((item) => presetRowGroup(item.row) === group),
  })).filter((section) => section.items.length > 0);
}

// ─── 按名称排：中文名按拼音首字母插进字母序（火山引擎排在 H）────────────────────

const collator = new Intl.Collator(["zh-Hans-u-co-pinyin", "en"], {
  sensitivity: "base",
  numeric: true,
});
const PINYIN_LETTERS = "abcdefghjklmnopqrstwxyz";
const PINYIN_BOUNDARIES = "阿八嚓哒妸发旮哈讥咔垃痳拏噢妑七呥扨它穵夕丫帀";
const HAN = /[㐀-鿿]/;

function sortKey(name: string): string {
  const first = name.charAt(0);
  if (!HAN.test(first)) return name.toLowerCase();
  let letter = "z";
  for (let i = PINYIN_BOUNDARIES.length - 1; i >= 0; i -= 1) {
    if (collator.compare(PINYIN_BOUNDARIES[i], first) <= 0) {
      letter = PINYIN_LETTERS[i];
      break;
    }
  }
  return `${letter}${name}`;
}

function sortByName<T>(items: T[], nameOf: (item: T) => string): T[] {
  return items
    .map((item) => ({ item, key: sortKey(nameOf(item)) }))
    .sort((a, b) => collator.compare(a.key, b.key))
    .map(({ item }) => item);
}

export function sortPresetsByName(
  entries: PresetEntry[],
  t: Translate,
): PresetEntry[] {
  return sortByName(entries, (entry) => presetDisplayName(entry.preset, t));
}

export function sortPresetRowsByName<T extends { row: PresetRowItem }>(
  items: T[],
  t: Translate,
): T[] {
  return sortByName(items, (item) => presetRowName(item.row, t));
}

/** 预设需要经过路由才能用（托管 OAuth、要转换格式的） */
export function presetNeedsRouting(
  appId: AppId | undefined,
  entry: PresetEntry,
): boolean {
  if (!appId) return false;
  const p = entry.preset as PresetFields;
  const provider = {
    id: entry.id,
    name: p.name,
    settingsConfig: (p as { settingsConfig?: Record<string, unknown> })
      .settingsConfig as Provider["settingsConfig"],
    category: p.category,
    meta: {
      ...(p.apiFormat ? { apiFormat: p.apiFormat } : {}),
      ...(p.providerType ? { providerType: p.providerType } : {}),
    },
  } as Provider;
  try {
    return providerNeedsRouting(appId, provider);
  } catch {
    return false;
  }
}

/** 合成的一行只有所有版本都要路由时才挂「需要路由」 */
export function presetRowNeedsRouting(
  appId: AppId | undefined,
  row: PresetRowItem,
): boolean {
  return row.versions.every((entry) => presetNeedsRouting(appId, entry));
}

/** 账号登录类的副行：用哪家的账号登录 */
export function loginAccountKey(
  appId: AppId | undefined,
  preset: AnyPreset,
): string {
  const type = (preset as PresetFields).providerType;
  if (type === "github_copilot") return "github";
  if (type === "codex_oauth") return "chatgpt";
  if (type === "xai_oauth") return "xai";
  switch (appId) {
    case "codex":
      return "chatgpt";
    case "gemini":
      return "google";
    case "grokbuild":
      return "xai";
    default:
      return "claude";
  }
}
