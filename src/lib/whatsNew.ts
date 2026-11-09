import { compareVersions } from "@/lib/version";

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

/** 官网更新日志：不带版本是列表页。官网只有 zh / en / ja，繁中用简中页面 */
export function changelogUrl(
  language: WhatsNewLanguage,
  version?: string,
): string {
  const siteLanguage = language === "zh-TW" ? "zh" : language;
  const base = `https://ccswitch.io/${siteLanguage}/changelog`;
  return version ? `${base}/${version}` : base;
}
