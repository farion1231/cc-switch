import type { UpdateInfo } from "@/lib/updater";

/** Read both Markdown announcements and the serialized summary in latest.json. */
export function getUpdateReleaseNotes(
  info: UpdateInfo,
  language: string,
): string | null {
  const notes = info.notes?.trim();
  if (!notes || /^Release\s+v?\d+\.\d+\.\d+(?:-[\w.-]+)?$/i.test(notes)) {
    return null;
  }
  let data: unknown;
  try {
    data = JSON.parse(notes);
  } catch {
    return notes;
  }
  if (typeof data !== "object" || data === null || Array.isArray(data)) {
    return null;
  }
  const entry = data as Record<string, unknown>;
  if (entry.version !== info.availableVersion.replace(/^v/, "")) return null;
  const locale = language === "zh-TW" ? "zh" : language.split("-")[0];
  if (typeof entry.releaseNotes === "object" && entry.releaseNotes !== null) {
    const releases = entry.releaseNotes as Record<string, unknown>;
    const markdown = releases[locale] ?? releases.en;
    if (typeof markdown === "string" && markdown.trim()) return markdown;
  }
  if (!Array.isArray(entry.items) || entry.items.length === 0) return null;
  const summaryLanguage = ["zh", "zh-TW", "en", "ja"].includes(language)
    ? language
    : "en";
  const items: string[] = [];
  for (const item of entry.items) {
    if (typeof item !== "object" || item === null) return null;
    const text: unknown = item[summaryLanguage];
    if (typeof text !== "string" || !text.trim()) return null;
    items.push(`- ${text}`);
  }
  return items.join("\n");
}
