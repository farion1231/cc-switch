import { compareVersions } from "@/lib/version";
import { settingsApi } from "@/lib/api/settings";

/**
 * 更新摘要：每个版本一份 `src/whats-new/<version>.json`，构建时打进安装包。
 * 新版安装包天然带着它自己和之前所有版本的摘要，弹窗不联网。格式见同目录 README。
 */

export const WHATS_NEW_ITEM_TYPES = ["new", "fix", "improve"] as const;
export type WhatsNewItemType = (typeof WHATS_NEW_ITEM_TYPES)[number];

export const WHATS_NEW_LANGUAGES = ["zh", "zh-TW", "en", "ja"] as const;
export type WhatsNewLanguage = (typeof WHATS_NEW_LANGUAGES)[number];

export type WhatsNewItem = { type: WhatsNewItemType } & Record<
  WhatsNewLanguage,
  string
>;

export interface WhatsNewEntry {
  version: string;
  /** 空数组 = 这一版不弹窗（纯构建、CI 之类的版本） */
  items: WhatsNewItem[];
  /** 待更新的远端版本（安装包里还没有这份摘要，联网取的） */
  pending?: boolean;
}

const modules = import.meta.glob<WhatsNewEntry>("../whats-new/*.json", {
  eager: true,
  import: "default",
});

/** 所有版本的摘要，新到旧 */
export const WHATS_NEW_ENTRIES: WhatsNewEntry[] = Object.values(modules).sort(
  (a, b) => compareVersions(b.version, a.version),
);

/** 当前版本比记录的已看版本新（或从没记录过）：需要处理一次 */
export function isNewerThanSeen(
  current: string,
  seen: string | undefined,
): boolean {
  return !seen || compareVersions(current, seen) > 0;
}

/**
 * 启动弹窗要显示的版本：`(seen, current]` 里有内容的版本，新到旧。
 * 没有记录（从还没有这个功能的版本升上来）时不知道起点，只显示当前版本。
 */
export function entriesSince(
  entries: WhatsNewEntry[],
  seen: string | undefined,
  current: string,
): WhatsNewEntry[] {
  return entries.filter((entry) => {
    if (entry.items.length === 0) return false;
    if (!seen) return entry.version === current;
    return (
      compareVersions(entry.version, seen) > 0 &&
      compareVersions(entry.version, current) <= 0
    );
  });
}

/** 关于页手动查看：不超过当前版本、有内容的版本，新到旧 */
export function entriesUpTo(
  entries: WhatsNewEntry[],
  current: string,
): WhatsNewEntry[] {
  return entries.filter(
    (entry) =>
      entry.items.length > 0 && compareVersions(entry.version, current) <= 0,
  );
}

/**
 * 看过之后要记下的版本：只往高记。降级后不把记录改低，
 * 再升回来时看过的版本不会重复出现。
 */
export function markSeen(
  seen: string | undefined,
  current: string | undefined,
): string | undefined {
  if (!current) return seen;
  return seen && compareVersions(seen, current) > 0 ? seen : current;
}

export function resolveWhatsNewLanguage(language: string): WhatsNewLanguage {
  return (WHATS_NEW_LANGUAGES as readonly string[]).includes(language)
    ? (language as WhatsNewLanguage)
    : "en";
}

/*
 * 待更新版本的随包摘要：安装包只带自己之前的摘要，新版本还没装进来，
 * 本地自然没有它的 JSON。发版时 release.yml 把 src/whats-new/<版本>.json
 * 附成发布资产（whats-new-<版本>.json），sync-r2 再镜像到 dl.ccswitch.io，
 * 后端按 tag 取摘要以避开 webview 的 CORS 限制，镜像在前、GitHub 在后。
 */
/** 远端摘要缓存进 localStorage，同一版本只取一次（存不下就每次都取） */
const REMOTE_CACHE_PREFIX = "ccswitch:whats-new:remote:";
const REMOTE_MAX_ITEMS = 4;

function readCachedRemoteEntry(version: string): WhatsNewEntry | null {
  try {
    const raw = localStorage.getItem(REMOTE_CACHE_PREFIX + version);
    if (!raw) return null;
    const entry = parseWhatsNewEntry(JSON.parse(raw) as unknown);
    return entry && entry.version === version && entry.items.length > 0
      ? entry
      : null;
  } catch {
    return null;
  }
}

function cacheRemoteEntry(version: string, entry: WhatsNewEntry): void {
  try {
    localStorage.setItem(REMOTE_CACHE_PREFIX + version, JSON.stringify(entry));
  } catch {
    // 存不下就只在本次会话里有效
  }
}

/**
 * 对任意取回来的数据只判形状、不抛错：弹窗渲染直接读 item.type、item[语言]，
 * 结构不对不能进列表。空 items 也不算摘要（纯构建版本本来就不弹窗）。
 */
export function parseWhatsNewEntry(data: unknown): WhatsNewEntry | null {
  if (typeof data !== "object" || data === null || Array.isArray(data)) {
    return null;
  }
  const record = data as Record<string, unknown>;
  if (
    typeof record.version !== "string" ||
    !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(record.version)
  ) {
    return null;
  }
  if (!Array.isArray(record.items) || record.items.length > REMOTE_MAX_ITEMS) {
    return null;
  }
  const items: WhatsNewItem[] = [];
  for (const item of record.items) {
    if (typeof item !== "object" || item === null || Array.isArray(item)) {
      return null;
    }
    const itemRecord = item as Record<string, unknown>;
    if (
      !(WHATS_NEW_ITEM_TYPES as readonly unknown[]).includes(itemRecord.type)
    ) {
      return null;
    }
    const texts = { type: itemRecord.type } as Partial<WhatsNewItem>;
    for (const language of WHATS_NEW_LANGUAGES) {
      const text = itemRecord[language];
      if (typeof text !== "string" || text.trim() === "") return null;
      texts[language] = text;
    }
    items.push(texts as WhatsNewItem);
  }
  return { version: record.version, items };
}

/**
 * 取待更新版本的摘要：同一版本先读 localStorage 缓存，没有才联网，
 * 逐个地址尝试（镜像在前）。失败、结构不对或版本不匹配一律返回 null，
 * 近期更新就退回只显示已装版本的摘要。
 */
export async function fetchWhatsNewSummary(
  version: string,
): Promise<WhatsNewEntry | null> {
  const cached = readCachedRemoteEntry(version);
  if (cached) return cached;

  for (const source of ["mirror", "github"] as const) {
    try {
      const data = await settingsApi.fetchWhatsNewSummary(version, source);
      const entry = parseWhatsNewEntry(data);
      if (entry && entry.version === version && entry.items.length > 0) {
        cacheRemoteEntry(version, entry);
        return entry;
      }
    } catch {
      // 网络失败就静默试下一个地址，最后退回本地摘要
    }
  }
  return null;
}

/** 官网更新日志：不带版本是列表页。官网只有 zh / en / ja，繁中用简中页面 */
export function changelogUrl(
  language: WhatsNewLanguage,
  version?: string,
): string {
  const siteLanguage = language === "zh-TW" ? "zh" : language;
  const base = `https://ccswitch.io/${siteLanguage}/changelog`;
  return version ? `${base}/${version}` : base;
}
