import { describe, expect, it } from "vitest";
import { matchesSearchQuery, normalizeSearchQuery } from "@/utils/search";

describe("search utilities", () => {
  it("normalizes surrounding whitespace and case", () => {
    expect(normalizeSearchQuery("  KIMI  ")).toBe("kimi");
  });

  it("matches empty queries and substrings case-insensitively", () => {
    expect(matchesSearchQuery("", "anything")).toBe(true);
    expect(matchesSearchQuery("  SON  ", "Claude Sonnet")).toBe(true);
    expect(matchesSearchQuery("opus", "Claude Sonnet")).toBe(false);
  });

  it("checks all candidates and ignores absent values", () => {
    expect(matchesSearchQuery("example.com", null, "https://example.com")).toBe(
      true,
    );
    expect(matchesSearchQuery("notes", undefined, "provider notes")).toBe(true);
    expect(matchesSearchQuery("missing", null, undefined)).toBe(false);
  });

  it("treats slashes and hyphens as ordinary searchable characters", () => {
    expect(
      matchesSearchQuery(
        "anthropic/claude-3-5",
        "openrouter/anthropic/claude-3-5-sonnet",
      ),
    ).toBe(true);
  });
});
