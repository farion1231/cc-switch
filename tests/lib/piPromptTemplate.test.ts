import { describe, expect, it } from "vitest";
import {
  getPiPromptTemplateDescription,
  getPiPromptTemplateSummary,
  setPiPromptTemplateDescription,
  stripPiPromptTemplateDescription,
} from "@/lib/piPromptTemplate";

describe("Pi prompt template metadata", () => {
  it("uses only the native description as the list note", () => {
    expect(getPiPromptTemplateSummary("Review the current changes.")).toEqual(
      {},
    );
    expect(
      getPiPromptTemplateSummary(
        "---\ndescription: Review changes\n---\nHidden body",
      ),
    ).toEqual({
      description: "Review changes",
      argumentHint: undefined,
    });
  });

  it("separates and restores description without dropping other frontmatter", () => {
    const document =
      '---\ndescription: "Review changes"\nargument-hint: "<target>"\ncustom: keep\n---\nReview $1';
    const editable = stripPiPromptTemplateDescription(document);

    expect(getPiPromptTemplateDescription(document)).toBe("Review changes");
    expect(editable).toBe(
      '---\nargument-hint: "<target>"\ncustom: keep\n---\nReview $1',
    );
    expect(setPiPromptTemplateDescription(editable, "Updated note")).toBe(
      '---\ndescription: "Updated note"\nargument-hint: "<target>"\ncustom: keep\n---\nReview $1',
    );
  });
});
