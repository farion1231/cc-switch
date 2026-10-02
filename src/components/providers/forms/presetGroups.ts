import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";
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

export function presetMatches(
  entry: PresetEntry,
  query: string,
  t: Translate,
): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  const haystack = [
    presetDisplayName(entry.preset, t),
    entry.preset.name,
    domainBody(presetDomain(entry.preset)),
    PRESET_SEARCH_ALIASES[entry.preset.name] ?? "",
  ]
    .join(" ")
    .toLowerCase();
  return needle
    .split(/\s+/)
    .every((part) => part.length > 0 && haystack.includes(part));
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

export function sortPresetsByName(
  entries: PresetEntry[],
  t: Translate,
): PresetEntry[] {
  return entries
    .map((entry) => ({
      entry,
      key: sortKey(presetDisplayName(entry.preset, t)),
    }))
    .sort((a, b) => collator.compare(a.key, b.key))
    .map(({ entry }) => entry);
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
