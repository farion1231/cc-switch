import { describe, expect, it } from "vitest";
import {
  buildInputDiff,
  parseDiff,
} from "@/components/sessions/reader/SessionDiff";
import type { ToolCallBlock } from "@/types";

describe("parseDiff", () => {
  it("preserves header-like content reconstructed from an Edit call", () => {
    const inputPreview = JSON.stringify({
      file_path: "/project/example.patch",
      old_string: "-- old",
      new_string: "++ new",
    });
    const call: ToolCallBlock = {
      type: "tool_call",
      id: "edit-patch",
      rawName: "Edit",
      kind: "edit",
      title: "example.patch",
      inputPreview,
      inputTotalLen: inputPreview.length,
    };

    const diff = buildInputDiff(call);

    expect(diff).toBe("@@\n--- old\n+++ new");
    expect(parseDiff(diff!)).toEqual([
      { type: "del", text: "-- old", oldNo: undefined },
      { type: "add", text: "++ new", newNo: undefined },
    ]);
  });

  it("preserves header-like lines inside a numbered unified hunk", () => {
    expect(
      parseDiff(
        "--- a/example.patch\n+++ b/example.patch\n@@ -3,2 +9,2 @@\n retained\n--- old\n+++ new\n",
      ),
    ).toEqual([
      { type: "file", path: "example.patch" },
      { type: "context", text: "retained", oldNo: 3, newNo: 9 },
      { type: "del", text: "-- old", oldNo: 4 },
      { type: "add", text: "++ new", newNo: 10 },
    ]);
  });

  it("recognizes the next file after a completed hunk without git headers", () => {
    expect(
      parseDiff(
        "--- a/first.txt\n+++ b/first.txt\n@@ -1 +1 @@\n-before\n+after\n--- a/second.txt\n+++ b/second.txt\n@@ -0,0 +1 @@\n+added\n",
      ),
    ).toEqual([
      { type: "file", path: "first.txt" },
      { type: "del", text: "before", oldNo: 1 },
      { type: "add", text: "after", newNo: 1 },
      { type: "file", path: "second.txt" },
      { type: "add", text: "added", newNo: 1 },
    ]);
  });

  it("keeps apply_patch file boundaries after unnumbered hunks", () => {
    expect(
      parseDiff(
        "*** Begin Patch\n*** Update File: example.patch\n@@\n--- old\n+++ new\n*** Add File: next.txt\n+added\n*** End Patch\n",
      ),
    ).toEqual([
      { type: "file", path: "example.patch" },
      { type: "del", text: "-- old", oldNo: undefined },
      { type: "add", text: "++ new", newNo: undefined },
      { type: "file", path: "next.txt" },
      { type: "add", text: "added", newNo: undefined },
    ]);
  });

  it("does not count no-newline markers as hunk content", () => {
    expect(
      parseDiff(
        "@@ -1,2 +4,2 @@\n-before\n+after\n\\ No newline at end of file\n--- old\n+++ new\n",
      ),
    ).toEqual([
      { type: "del", text: "before", oldNo: 1 },
      { type: "add", text: "after", newNo: 4 },
      { type: "del", text: "-- old", oldNo: 2 },
      { type: "add", text: "++ new", newNo: 5 },
    ]);
  });

  it("finishes insertion-only and deletion-only hunks before the next file", () => {
    expect(
      parseDiff(
        "--- a/first.txt\n+++ b/first.txt\n@@ -0,0 +1 @@\n+added\n--- a/second.txt\n+++ b/second.txt\n@@ -1 +0,0 @@\n-removed\n--- a/third.txt\n+++ b/third.txt\n@@ -2 +3 @@\n-before\n+after\n",
      ),
    ).toEqual([
      { type: "file", path: "first.txt" },
      { type: "add", text: "added", newNo: 1 },
      { type: "file", path: "second.txt" },
      { type: "del", text: "removed", oldNo: 1 },
      { type: "file", path: "third.txt" },
      { type: "del", text: "before", oldNo: 2 },
      { type: "add", text: "after", newNo: 3 },
    ]);
  });
});
