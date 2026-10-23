import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

// Environment detection & install management for DSH ship in a follow-up PR;
// this file only covers surfaces that are part of provider switching.

describe("DeepSeek Harness visible surfaces", () => {
  it("shows the active app name above the provider list", () => {
    const source = fs.readFileSync(
      path.resolve(__dirname, "../../src/App.tsx"),
      "utf8",
    );
    expect(source).toContain("title={APP_DISPLAY_NAME[activeApp]}");
    const glyphs = fs.readFileSync(
      path.resolve(__dirname, "../../src/components/shell/AppGlyph.tsx"),
      "utf8",
    );
    expect(glyphs).toContain('"deepseek-harness": "DeepSeek Harness"');
  });
});
