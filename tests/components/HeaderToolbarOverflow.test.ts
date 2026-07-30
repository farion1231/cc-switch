import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const APP_TSX = path.resolve(__dirname, "..", "..", "src", "App.tsx");
const INDEX_CSS = path.resolve(__dirname, "..", "..", "src", "index.css");

describe("Header toolbar overflow (#5789)", () => {
  const appSource = fs.readFileSync(APP_TSX, "utf8");
  const cssSource = fs.readFileSync(INDEX_CSS, "utf8");

  it("lets the provider toolbar scroll horizontally instead of clipping", () => {
    expect(appSource).toContain(
      'className="toolbar-x-scroll flex flex-1 min-w-0 items-center py-4 pr-2"',
    );
    expect(appSource).not.toContain(
      'className="flex flex-1 min-w-0 overflow-x-hidden items-center py-4 pr-2"',
    );
    expect(cssSource).toContain(".toolbar-x-scroll");
    expect(cssSource).toMatch(
      /\.toolbar-x-scroll\s*\{[^}]*overflow-x:\s*auto/s,
    );
    // Opt back into a thin scrollbar despite the global scrollbar:none rule.
    expect(cssSource).toContain(".toolbar-x-scroll::-webkit-scrollbar");
    expect(cssSource).toContain("scrollbar-width: thin");
    // Tauri drag region on the header would steal scrollbar clicks without
    // an explicit no-drag on the scroll container.
    expect(appSource).toMatch(
      /toolbar-x-scroll[\s\S]{0,120}WebkitAppRegion:\s*"no-drag"/,
    );
  });

  it("keeps the add-provider action outside the scroll clip region", () => {
    // Pin the orange "+" outside the scroll container so narrow windows can
    // always open Add Provider. ProfileSwitcher may sit before the scroll
    // region; the add button must follow the toolbar-x-scroll container.
    expect(appSource).toMatch(
      /toolbar-x-scroll[\s\S]*?\n\s*\{currentView === "providers" && \(\s*\n\s*<div\s*\n\s*className="flex shrink-0 items-center"[\s\S]*?setIsAddOpen\(true\)[\s\S]*?header\.addProvider/,
    );
  });

  it("preserves upstream profile switcher placement before the scroll region", () => {
    const profileIdx = appSource.indexOf("<ProfileSwitcher");
    const scrollIdx = appSource.indexOf("toolbar-x-scroll");
    const addIdx = appSource.lastIndexOf("setIsAddOpen(true)");
    expect(profileIdx).toBeGreaterThan(-1);
    expect(scrollIdx).toBeGreaterThan(profileIdx);
    expect(addIdx).toBeGreaterThan(scrollIdx);
  });
});
