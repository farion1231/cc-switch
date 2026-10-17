import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";

const stylesheet = document.createElement("style");
stylesheet.textContent = readFileSync(
  resolve(__dirname, "../../src/tray-panel/tray-panel.css"),
  "utf8",
);
const root = document.documentElement;
const originalClassName = root.className;
const originalGlass = root.dataset.glass;
const originalLayout = root.dataset.layout;
let canvas: HTMLDivElement;

beforeAll(() => document.head.append(stylesheet));
afterAll(() => stylesheet.remove());
afterEach(() => {
  canvas.remove();
  root.className = originalClassName;
  if (originalGlass === undefined) delete root.dataset.glass;
  else root.dataset.glass = originalGlass;
  if (originalLayout === undefined) delete root.dataset.layout;
  else root.dataset.layout = originalLayout;
});

function cardFor(platform: "windows" | "macos", dark: boolean) {
  root.classList.toggle("dark", dark);
  root.dataset.glass = platform === "macos" ? "native" : "frosted";
  root.dataset.layout = platform === "macos" ? "floating" : "joined";
  canvas = document.createElement("div");
  canvas.dataset.platform = platform;
  const card = document.createElement("div");
  card.className = "tp-card";
  canvas.append(card);
  document.body.append(canvas);
  return getComputedStyle(card);
}

describe("TrayPanel backgrounds stay scoped to each platform", () => {
  it.each([false, true])("keeps Windows cards opaque (dark=%s)", (dark) => {
    const style = cardFor("windows", dark);
    expect(style.backgroundColor).toBe(
      dark ? "rgb(36, 36, 40)" : "rgb(235, 235, 237)",
    );
    expect(getComputedStyle(root).backgroundColor).toBe("rgba(0, 0, 0, 0)");
  });

  it.each([false, true])(
    "preserves the macOS overlay on native glass (dark=%s)",
    (dark) => {
      const style = cardFor("macos", dark);
      expect(style.backgroundColor).toBe(
        dark ? "rgba(24, 24, 27, 0.08)" : "rgba(255, 255, 255, 0.06)",
      );
      expect(style.boxShadow).toContain("0 6px 16px");
      expect(getComputedStyle(root).backgroundColor).toBe("rgba(0, 0, 0, 0)");
    },
  );
});
