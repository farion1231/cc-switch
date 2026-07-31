import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const SELECT_TSX = path.resolve(
  __dirname,
  "..",
  "..",
  "src",
  "components",
  "ui",
  "select.tsx",
);

describe("SelectContent scroll bounds", () => {
  const source = fs.readFileSync(SELECT_TSX, "utf8");

  it("limits popper content to the available viewport height", () => {
    expect(source).toContain("--radix-select-content-available-height");
    expect(source).toContain(
      "max-h-[min(24rem,var(--radix-select-content-available-height))]",
    );
  });

  it("allows long option lists to scroll vertically", () => {
    expect(source).toContain("overflow-y-auto");
    expect(source).toContain("overflow-x-hidden");
  });
});
