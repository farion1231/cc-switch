import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

describe("DeepSeek Harness visible surfaces", () => {
  it("appears in the local environment tool matrix", () => {
    const source = fs.readFileSync(
      path.resolve(__dirname, "../../src/components/apps/useToolManagement.ts"),
      "utf8",
    );
    expect(source).toContain('"dsh"');
    expect(source).toContain('dsh: "DeepSeek Harness"');
    expect(source).toContain('dsh: "deepseek-harness"');
  });

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
