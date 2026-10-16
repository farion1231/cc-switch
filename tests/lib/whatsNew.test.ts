import { describe, expect, it } from "vitest";
import {
  changelogUrl,
  entriesSince,
  entriesUpTo,
  isNewerThanSeen,
  markSeen,
  parseWhatsNewEntry,
  parseWhatsNewNotes,
  resolveWhatsNewLanguage,
  type WhatsNewEntry,
} from "@/lib/whatsNew";

const item = (text: string) => ({
  type: "fix" as const,
  zh: text,
  "zh-TW": text,
  en: text,
  ja: text,
});

// 新到旧，和 WHATS_NEW_ENTRIES 的顺序一致
const ENTRIES: WhatsNewEntry[] = [
  { version: "4.1.0", items: [item("future")] },
  { version: "4.0.4", items: [item("four")] },
  { version: "4.0.3", items: [] },
  { version: "4.0.2", items: [item("two")] },
  { version: "4.0.1", items: [item("one")] },
];

const versions = (entries: WhatsNewEntry[]) => entries.map((e) => e.version);

describe("entriesSince", () => {
  it("returns versions after the seen one up to the current one, skipping empty ones", () => {
    expect(versions(entriesSince(ENTRIES, "4.0.1", "4.0.4"))).toEqual([
      "4.0.4",
      "4.0.2",
    ]);
  });

  it("shows only the current version when nothing was recorded", () => {
    expect(versions(entriesSince(ENTRIES, undefined, "4.0.2"))).toEqual([
      "4.0.2",
    ]);
    expect(entriesSince(ENTRIES, undefined, "4.0.3")).toEqual([]);
  });

  it("returns nothing on a downgrade", () => {
    expect(entriesSince(ENTRIES, "4.0.4", "4.0.2")).toEqual([]);
  });

  it("treats a prerelease as older than its release", () => {
    expect(versions(entriesSince(ENTRIES, "4.0.2-beta.1", "4.0.2"))).toEqual([
      "4.0.2",
    ]);
  });
});

describe("entriesUpTo", () => {
  it("lists non-empty versions up to the current one", () => {
    expect(versions(entriesUpTo(ENTRIES, "4.0.4"))).toEqual([
      "4.0.4",
      "4.0.2",
      "4.0.1",
    ]);
  });
});

describe("seen version", () => {
  it("only moves forward", () => {
    expect(markSeen(undefined, "4.0.2")).toBe("4.0.2");
    expect(markSeen("4.0.1", "4.0.2")).toBe("4.0.2");
    expect(markSeen("4.0.4", "4.0.2")).toBe("4.0.4");
    expect(markSeen("4.0.1", undefined)).toBe("4.0.1");
  });

  it("is due only when the current version is newer", () => {
    expect(isNewerThanSeen("4.0.2", undefined)).toBe(true);
    expect(isNewerThanSeen("4.0.2", "4.0.1")).toBe(true);
    expect(isNewerThanSeen("4.0.2", "4.0.2")).toBe(false);
    expect(isNewerThanSeen("4.0.2", "4.0.4")).toBe(false);
  });
});

describe("language and links", () => {
  it("falls back to English for unknown languages", () => {
    expect(resolveWhatsNewLanguage("zh-TW")).toBe("zh-TW");
    expect(resolveWhatsNewLanguage("ja")).toBe("ja");
    expect(resolveWhatsNewLanguage("fr")).toBe("en");
  });

  it("links to the website changelog, Traditional Chinese uses the zh pages", () => {
    expect(changelogUrl("en", "4.0.2")).toBe(
      "https://ccswitch.io/en/changelog/4.0.2",
    );
    expect(changelogUrl("zh-TW")).toBe("https://ccswitch.io/zh/changelog");
  });
});

const REMOTE_ENTRY = {
  version: "4.0.13",
  items: [{ type: "new", zh: "新", "zh-TW": "新", en: "new", ja: "新機能" }],
};

describe("parseWhatsNewEntry", () => {
  it("accepts a well-formed entry and keeps only the four languages", () => {
    const parsed = parseWhatsNewEntry(REMOTE_ENTRY);
    expect(parsed).toEqual({
      version: "4.0.13",
      items: [
        {
          type: "new",
          zh: "新",
          "zh-TW": "新",
          en: "new",
          ja: "新機能",
        },
      ],
    });
  });

  it("rejects broken shapes instead of throwing", () => {
    expect(parseWhatsNewEntry(null)).toBeNull();
    expect(parseWhatsNewEntry([])).toBeNull();
    expect(parseWhatsNewEntry({ version: 3, items: [] })).toBeNull();
    expect(
      parseWhatsNewEntry({ version: "4.0.2-beta.1-bad/..", items: [] }),
    ).toBeNull();
    expect(parseWhatsNewEntry({ version: "4.0.13" })).toBeNull();
    expect(parseWhatsNewEntry({ version: "4.0.13", items: [null] })).toBeNull();
    expect(
      parseWhatsNewEntry({ version: "4.0.13", items: [{ type: "news" }] }),
    ).toBeNull();
    expect(
      parseWhatsNewEntry({
        version: "4.0.13",
        items: [{ type: "new", zh: "新", "zh-TW": "新", en: "new" }],
      }),
    ).toBeNull();
    expect(
      parseWhatsNewEntry({
        version: "4.0.13",
        items: [
          { type: "new", zh: "", "zh-TW": "新", en: "new", ja: "新機能" },
        ],
      }),
    ).toBeNull();
  });

  it("allows empty items and rejects more than four", () => {
    // 纯构建版本本来就是空 items，形状合法；是否显示由调用方决定
    expect(parseWhatsNewEntry({ version: "4.0.13", items: [] })).toEqual({
      version: "4.0.13",
      items: [],
    });
    expect(
      parseWhatsNewEntry({
        version: "4.0.13",
        items: Array.from({ length: 5 }, () => ({
          type: "new",
          zh: "新",
          "zh-TW": "新",
          en: "new",
          ja: "新機能",
        })),
      }),
    ).toBeNull();
  });
});

describe("parseWhatsNewNotes", () => {
  it("reads the serialized summary from the updater body", () => {
    expect(parseWhatsNewNotes(JSON.stringify(REMOTE_ENTRY), "4.0.13")).toEqual(
      REMOTE_ENTRY,
    );
  });

  it.each([
    undefined,
    "",
    "Release v4.0.13",
    '{"version":',
    "null",
    "[]",
    JSON.stringify({ version: "4.0.13" }),
    JSON.stringify({ version: "4.0.11", items: REMOTE_ENTRY.items }),
    JSON.stringify({ version: "4.0.13", items: [] }),
    JSON.stringify({ version: "4.0.13", items: [{ type: "new" }] }),
  ])("returns no remote summary for unusable notes: %s", (notes) => {
    expect(parseWhatsNewNotes(notes, "4.0.13")).toBeNull();
  });
});
