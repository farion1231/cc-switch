import { describe, expect, it } from "vitest";
import { buildInputDiff } from "@/components/sessions/reader/SessionDiff";
import { call } from "./helpers";

const multiEdit = (edits: unknown) => {
  const inputPreview = JSON.stringify({ path: "/tmp/file", edits });
  return call("agy:1:0", "edit", {
    rawName: "multi_replace_file_content",
    inputPreview,
    inputTotalLen: inputPreview.length,
  });
};

describe("AGY multi-replacement diff fallback", () => {
  it("rejects the incomplete chunk shape found in local exports", () => {
    expect(
      buildInputDiff(
        multiEdit([
          { old_string: "old", new_string: "new" },
          { new_string: "more", EndLine: 2, AllowMultiple: false },
        ]),
      ),
    ).toBeNull();
  });

  it("rejects empty, truncated and non-object chunks", () => {
    for (const edits of [[], "[{<truncated>", [null], ["invalid"]]) {
      expect(buildInputDiff(multiEdit(edits))).toBeNull();
    }
  });

  it("keeps complete replacements and explicit empty sides", () => {
    expect(
      buildInputDiff(
        multiEdit([
          { old_string: "old", new_string: "new" },
          { old_string: "", new_string: "added" },
          { old_string: "removed", new_string: "" },
        ]),
      ),
    ).toBe("@@\n-old\n+new\n@@\n+added\n@@\n-removed");
  });

  it("preserves other tools' existing insertion fallback", () => {
    const inputPreview = JSON.stringify({ new_string: "added" });
    expect(
      buildInputDiff(
        call("other", "edit", {
          rawName: "replace",
          inputPreview,
          inputTotalLen: inputPreview.length,
        }),
      ),
    ).toBe("@@\n+added");
  });
});
