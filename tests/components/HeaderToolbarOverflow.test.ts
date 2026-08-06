import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const APP_TSX = path.resolve(__dirname, "..", "..", "src", "App.tsx");

describe("Header toolbar overflow (#5789)", () => {
  const appSource = fs.readFileSync(APP_TSX, "utf8");

  it("keeps AppSwitcher in a width-bounded middle region", () => {
    expect(appSource).toContain(
      'className="flex flex-1 min-w-0 items-center justify-end overflow-hidden py-4"',
    );
    expect(appSource).toMatch(
      /overflow-hidden py-4">\s*\{currentView === "providers" && \(\s*<AppSwitcher/,
    );
  });

  it("keeps primary header actions in a fixed shrink-0 cluster", () => {
    expect(appSource).toContain(
      'className="flex shrink-0 items-center py-4"',
    );
    // Add Provider lives with the fixed-right actions, not in a clipped scroll region.
    expect(appSource).toMatch(
      /flex shrink-0 items-center py-4[\s\S]*?setIsAddOpen\(true\)[\s\S]*?header\.addProvider/,
    );
  });

  it("does not reintroduce a clipping overflow-x-hidden toolbar wrapper", () => {
    expect(appSource).not.toContain(
      'className="flex flex-1 min-w-0 overflow-x-hidden items-center py-4 pr-2"',
    );
    expect(appSource).not.toContain(
      'className="toolbar-x-scroll flex flex-1 min-w-0 items-center py-4 pr-2"',
    );
  });
});
