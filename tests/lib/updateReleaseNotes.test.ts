import { describe, expect, it } from "vitest";
import { getUpdateReleaseNotes } from "@/lib/updateReleaseNotes";
import summary from "@/whats-new/4.0.14.json";
import announcement from "../../docs/release-notes/v4.0.14-en.md?raw";

const info = { currentVersion: "4.0.13", availableVersion: "4.0.14" };

describe("updater release announcements", () => {
  it("reads full Markdown from serialized update notes with language fallback", () => {
    const notes = JSON.stringify({
      ...summary,
      releaseNotes: { en: announcement, zh: "# 中文公告" },
    });
    expect(getUpdateReleaseNotes({ ...info, notes }, "en")).toBe(announcement);
    expect(getUpdateReleaseNotes({ ...info, notes }, "tr")).toBe(announcement);
    expect(getUpdateReleaseNotes({ ...info, notes }, "zh-TW")).toBe(
      "# 中文公告",
    );
  });

  it("renders summary-only manifests in the selected language", () => {
    const notes = JSON.stringify(summary);
    expect(getUpdateReleaseNotes({ ...info, notes }, "zh-TW")).toBe(
      summary.items.map((item) => `- ${item["zh-TW"]}`).join("\n"),
    );
    expect(getUpdateReleaseNotes({ ...info, notes }, "tr")).toBe(
      summary.items.map((item) => `- ${item.en}`).join("\n"),
    );
  });

  it("continues to accept existing Markdown announcements", () => {
    expect(getUpdateReleaseNotes({ ...info, notes: announcement }, "en")).toBe(
      announcement.trim(),
    );
  });

  it.each([
    undefined,
    "",
    "Release v4.0.14",
    "Release 4.0.14",
    "null",
    "[]",
    "{}",
    JSON.stringify({ ...summary, version: "4.0.13" }),
    JSON.stringify({ version: "4.0.14", items: [{ en: 123 }] }),
  ])("ignores missing, placeholder or invalid notes: %s", (notes) => {
    expect(getUpdateReleaseNotes({ ...info, notes }, "en")).toBeNull();
  });
});
