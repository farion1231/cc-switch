import { describe, expect, it } from "vitest";
import { isValidPiPromptTemplateSlug } from "@/lib/piPromptSlug";

describe("isValidPiPromptTemplateSlug", () => {
  it("accepts callable Unicode and dotted slugs", () => {
    expect(isValidPiPromptTemplateSlug("review-pr")).toBe(true);
    expect(isValidPiPromptTemplateSlug("release.v2")).toBe(true);
    expect(isValidPiPromptTemplateSlug("评审")).toBe(true);
    expect(isValidPiPromptTemplateSlug("SYSTEM")).toBe(true);
  });

  it("rejects whitespace, portable filename hazards, and Windows device names", () => {
    for (const slug of [
      "release notes",
      "tab\tname",
      "bad:name",
      "bad*name",
      "CON",
      "con.anything",
      "LPT9",
      "nul.json",
    ]) {
      expect(isValidPiPromptTemplateSlug(slug), slug).toBe(false);
    }
  });

  it("measures the contract limit in UTF-8 bytes", () => {
    expect(isValidPiPromptTemplateSlug("a".repeat(128))).toBe(true);
    expect(isValidPiPromptTemplateSlug("a".repeat(129))).toBe(false);
    expect(isValidPiPromptTemplateSlug("评".repeat(42))).toBe(true);
    expect(isValidPiPromptTemplateSlug("评".repeat(43))).toBe(false);
  });
});
