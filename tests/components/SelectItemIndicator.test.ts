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

const stripComments = (source: string) =>
  source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:])\/\/.*$/gm, "$1");

describe("SelectItem indicator structure", () => {
  const source = stripComments(fs.readFileSync(SELECT_TSX, "utf8"));

  it("renders an item indicator before item text", () => {
    const indicatorIndex = source.indexOf("SelectPrimitive.ItemIndicator");
    const itemTextIndex = source.indexOf("SelectPrimitive.ItemText");

    expect(indicatorIndex).toBeGreaterThan(-1);
    expect(itemTextIndex).toBeGreaterThan(-1);
    expect(indicatorIndex).toBeLessThan(itemTextIndex);
  });

  it("wraps the indicator in the first direct span", () => {
    const itemTextIndex = source.indexOf("SelectPrimitive.ItemText");
    const beforeItemText = source.slice(0, itemTextIndex);
    const lastSpanOpen = beforeItemText.lastIndexOf("<span");
    const lastIndicator = beforeItemText.lastIndexOf("ItemIndicator");

    expect(lastSpanOpen).toBeGreaterThan(-1);
    expect(lastSpanOpen).toBeLessThan(lastIndicator);
  });
});
